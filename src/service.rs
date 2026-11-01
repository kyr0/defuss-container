use crate::{model::*, oci, process, provider::Engine, store::Store, util};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

pub struct Service {
    pub root: PathBuf,
    pub store: Store,
    pub engine: Engine,
    pub gate: Arc<Semaphore>,
    register_lock: std::sync::Mutex<()>,
}
impl Service {
    pub fn open(root: PathBuf, engine: PathBuf) -> Result<Self> {
        util::private_dir(&root)?;
        for d in ["apps", "jobs", "cache", "builds", "backup-meta"] {
            util::private_dir(&root.join(d))?;
        }
        let store = Store::open(&root)?;
        store.recover_jobs()?;
        Ok(Self {
            root,
            store,
            engine: Engine {
                executable: engine,
                exec_wait: Duration::from_secs(15),
            },
            gate: Arc::new(Semaphore::new(2)),
            register_lock: std::sync::Mutex::new(()),
        })
    }
    pub fn app_dir(&self, name: &str) -> PathBuf {
        self.root.join("apps").join(name)
    }
    pub fn register(&self, mut spec: AppSpec) -> Result<Application> {
        let _guard = self.register_lock.lock().unwrap();
        spec.validate()?;
        ensure!(
            !self.store.apps()?.iter().any(|a| a.spec.name == spec.name),
            "application already exists"
        );
        for a in self.store.apps()? {
            for p in &spec.ports {
                ensure!(
                    !a.spec.ports.iter().any(|v| v.host == p.host),
                    "host port already allocated: {}",
                    p.host
                );
            }
        }
        util::ensure_no_symlinks(&spec.mounts)?;
        fs::create_dir_all(&spec.mounts)?;
        spec.mounts = dunce::canonicalize(&spec.mounts)?;
        // Keep user shares outside private daemon state. A guest must not see tokens or other apps.
        ensure!(
            !spec.mounts.starts_with(&self.root) && !self.root.starts_with(&spec.mounts),
            "mount directory and daemon state must be disjoint"
        );
        for d in ["input", "output", "etc"] {
            let p = spec.mounts.join(d);
            util::ensure_no_symlinks(&p)?;
            fs::create_dir_all(p)?;
        }
        let dir = self.app_dir(&spec.name);
        util::private_dir(&dir)?;
        for d in ["control", "releases", "backups"] {
            util::private_dir(&dir.join(d))?;
        }
        let app = Application {
            spec,
            machine: format!("defuss-{}", uuid::Uuid::new_v4().simple()),
            active_release: None,
            previous_release: None,
            desired_running: false,
            last_error: None,
        };
        self.store.insert_app(&app)?;
        Ok(app)
    }
    pub fn enqueue(self: &Arc<Self>, name: &str, action: Action) -> Result<Job> {
        self.store.app(name)?;
        action.validate(name)?;
        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            app: name.into(),
            action,
            state: "queued".into(),
            created_at: util::now(),
            completed_at: None,
            result: None,
            error: None,
        };
        self.store
            .insert_job(&job)
            .context("another operation is active for this app")?;
        let this = self.clone();
        let mut j = job.clone();
        tokio::spawn(async move {
            let Ok(permit) = this.gate.clone().acquire_owned().await else {
                // Shutdown has drained the workers. Startup recovery handles queued jobs.
                return;
            };
            let owner = this.clone();
            let task = tokio::task::spawn_blocking(move || {
                j.state = "running".into();
                owner.store.save_job(&j)?;
                tracing::info!(job = %j.id, app = %j.app, action = ?j.action, "job started");
                let result = owner.perform(&j);
                j.completed_at = Some(util::now());
                match result {
                    Ok(v) => {
                        j.state = "succeeded".into();
                        j.result = Some(v);
                        tracing::info!(job = %j.id, app = %j.app, "job succeeded");
                    }
                    Err(e) => {
                        j.state = "failed".into();
                        j.error = Some(format!("{e:#}"));
                        tracing::warn!(job = %j.id, app = %j.app, error = %format!("{e:#}"), "job failed");
                        if let Ok(mut a) = owner.store.app(&j.app) {
                            a.last_error = j.error.clone();
                            let _ = owner.store.save_app(&a);
                        }
                    }
                }
                owner.store.save_job(&j)
            })
            .await;
            if let Err(e) = task {
                tracing::error!(error=%e,"job worker panicked; restart marks it interrupted");
            }
            drop(permit);
        });
        Ok(job)
    }
    fn perform(&self, j: &Job) -> Result<Value> {
        self.engine.verify()?;
        let log = self.root.join("jobs").join(format!("{}.log", j.id));
        let mut app = self.store.app(&j.app)?;
        let value = match &j.action {
            Action::Build => serde_json::to_value(self.build(&app, &log)?)?,
            Action::Deploy { release } => self.deploy(&mut app, release, &log)?,
            Action::Rollback => {
                let release = app
                    .previous_release
                    .clone()
                    .context("no previous release")?;
                self.deploy(&mut app, &release, &log)?
            }
            Action::Start => {
                ensure!(app.active_release.is_some(), "deploy a release first");
                self.write_launch(&app, app.active_release.as_deref().unwrap())?;
                let running = self.engine.is_running(&app.machine)?;
                let workload_running = running
                    && self
                        .engine
                        .exec(
                            &app.machine,
                            &[
                                crate::BUN.into(),
                                "/opt/defuss/supervisor.mjs".into(),
                                "probe".into(),
                            ],
                            10,
                            None,
                        )
                        .ok()
                        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                        .is_some_and(|v| {
                            v["state"] == "running"
                                && v["release"] == app.active_release.as_deref().unwrap()
                        });
                if !workload_running {
                    self.engine.stop(
                        &app.machine,
                        app.spec.stop_timeout_seconds + 5,
                        Some(&log),
                    )?;
                    self.engine
                        .machine("start", &app.machine, &[], 180, Some(&log))?;
                }
                self.wait_ready(&app, &log)?;
                app.desired_running = true;
                json!({"running":true})
            }
            Action::Stop => {
                self.engine
                    .stop(&app.machine, app.spec.stop_timeout_seconds + 5, Some(&log))?;
                app.desired_running = false;
                json!({"running":false})
            }
            Action::Backup => self.backup(&mut app, &log)?,
            Action::Restore { backup } => self.restore(&mut app, backup, &log)?,
            Action::Delete { .. } => {
                self.engine
                    .stop(&app.machine, app.spec.stop_timeout_seconds + 5, Some(&log))?;
                self.engine.delete(&app.machine, Some(&log))?;
                self.store.delete_app(&app.spec.name)?;
                return Ok(
                    json!({"deleted":true,"retained_artifacts":self.app_dir(&app.spec.name),"note":"Host shares and backup archives retained; VM disk deleted."}),
                );
            }
        };
        app.last_error = None;
        self.store.save_app(&app)?;
        Ok(value)
    }
    fn build(&self, app: &Application, log: &Path) -> Result<Release> {
        let id = uuid::Uuid::new_v4().to_string();
        let work = tempfile::tempdir_in(self.root.join("builds"))?;
        let git = work.path().join("git");
        fs::create_dir(&git)?;
        let gitcmd = |args: Vec<String>| {
            process::checked(
                "git",
                &args,
                Some(&git),
                Duration::from_secs(300),
                Some(log),
            )
        };
        gitcmd(vec![
            "-c".into(),
            "core.hooksPath=/dev/null".into(),
            "init".into(),
            "--bare".into(),
        ])?;
        gitcmd(vec![
            "-c".into(),
            "core.hooksPath=/dev/null".into(),
            "-c".into(),
            "protocol.file.allow=never".into(),
            "-c".into(),
            "http.followRedirects=false".into(),
            "fetch".into(),
            "--depth=1".into(),
            "--no-tags".into(),
            "--".into(),
            app.spec.repository.clone(),
            app.spec.git_ref.clone(),
        ])?;
        let commit = gitcmd(vec![
            "rev-parse".into(),
            "--verify".into(),
            "FETCH_HEAD^{commit}".into(),
        ])?
        .trim()
        .to_string();
        ensure!(
            commit.len() == 40 || commit.len() == 64,
            "invalid Git commit hash"
        );
        let source = work.path().join("source");
        fs::create_dir(&source)?;
        let tree = if app.spec.subdirectory.is_empty() {
            commit.clone()
        } else {
            format!("{commit}:{}", app.spec.subdirectory)
        };
        gitcmd(vec![
            "archive".into(),
            "--format=tar".into(),
            format!("--output={}", source.join("source.tar").display()),
            tree,
        ])?;
        ensure!(
            fs::metadata(source.join("source.tar"))?.len() <= 512 * 1024 * 1024,
            "source archive exceeds 512 MiB"
        );
        let helper = work.path().join("helper");
        fs::create_dir(&helper)?;
        fs::write(helper.join("build.mjs"), crate::BUILDER)?;
        fs::write(helper.join("archive.mjs"), crate::ARCHIVE_JS)?;
        fs::write(helper.join("policy.mjs"), crate::POLICY_JS)?;
        let release_dir = self.app_dir(&app.spec.name).join("releases").join(&id);
        util::private_dir(&release_dir)?;
        let base = oci::pull(&self.root.join("cache"))?;
        let builder = format!("defuss-build-{}", uuid::Uuid::new_v4().simple());
        let args = vec![
            "machine".into(),
            "create".into(),
            "--name".into(),
            builder.clone(),
            "--image".into(),
            format!("oven/bun@{}", base.digest),
            "--net".into(),
            "--net-backend".into(),
            "virtio-net".into(),
            "--block-private".into(),
            "--cpus".into(),
            app.spec.cpus.to_string(),
            "--mem".into(),
            app.spec.memory_mib.to_string(),
            "--storage".into(),
            app.spec.storage_gib.to_string(),
            "--user".into(),
            "0:0".into(),
            "--volume".into(),
            format!("{}:/source:ro", source.display()),
            "--volume".into(),
            format!("{}:/helper:ro", helper.display()),
            "--volume".into(),
            format!("{}:/export:rw", release_dir.display()),
        ];
        // Always clean the build VM, including a partially successful create/start.
        let result = (|| -> Result<()> {
            self.engine.call(&args, 300, Some(log))?;
            self.engine.start_no_workload(&builder, 300, Some(log))?;
            self.engine.exec(
                &builder,
                &[
                    crate::BUN.into(),
                    "/helper/build.mjs".into(),
                    id.clone(),
                    app.spec.build.to_string(),
                ],
                900,
                Some(log),
            )?;
            Ok(())
        })();
        let cleanup = self.engine.delete(&builder, Some(log));
        match (result, cleanup) {
            (Err(e), Err(c)) => {
                return Err(e.context(format!("builder {builder} cleanup also failed: {c:#}")));
            }
            (Err(e), _) => return Err(e),
            (_, Err(c)) => return Err(c.context(format!("builder {builder} cleanup failed"))),
            _ => {}
        }
        let layer = release_dir.join("app.tar");
        ensure!(
            fs::symlink_metadata(&layer)?.file_type().is_file(),
            "builder app layer must be a regular file, not a symlink"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                fs::metadata(&layer)?.nlink() == 1,
                "hardlinked app layer refused"
            );
        }
        // Remove every builder-controlled output except the checked app layer before writing host artifacts.
        for entry in fs::read_dir(&release_dir)? {
            let entry = entry?;
            if entry.file_name() != "app.tar" {
                let p = entry.path();
                if entry.file_type()?.is_dir() {
                    fs::remove_dir_all(p)?;
                } else {
                    fs::remove_file(p)?;
                }
            }
        }
        validate_app_layer(&layer, &id)?;
        let image_sha256 = oci::assemble(&base, &id, &layer, &release_dir.join("image.oci.tar"))?;
        let release = Release {
            id,
            app: app.spec.name.clone(),
            commit,
            base_digest: base.digest,
            architecture: base.architecture,
            layer_sha256: util::hash_file(&layer)?,
            image_sha256,
            created_at: util::now(),
        };
        util::write_json(&release_dir.join("release.json"), &release)?;
        self.store.add_release(&release)?;
        Ok(release)
    }
    fn write_launch(&self, app: &Application, release: &str) -> Result<()> {
        let r = self.store.release(release)?;
        ensure!(r.app == app.spec.name, "release belongs to another app");
        util::write_json(
            &self.app_dir(&app.spec.name).join("control/launch.json"),
            &json!({"app":app.spec.name,"release":release,"sha256":r.layer_sha256,"env":app.spec.env,"stop_timeout_seconds":app.spec.stop_timeout_seconds}),
        )
    }
    fn deploy(&self, app: &mut Application, id: &str, log: &Path) -> Result<Value> {
        let release = self.store.release(id)?;
        ensure!(
            release.app == app.spec.name,
            "release belongs to another app"
        );
        let dir = self.app_dir(&app.spec.name).join("releases").join(id);
        ensure!(
            util::hash_file(&dir.join("app.tar"))? == release.layer_sha256,
            "release layer corrupted"
        );
        if let Some(active) = &app.active_release {
            ensure!(
                self.store.release(active)?.base_digest == release.base_digest,
                "base image digest changed: create a fresh machine and restore a backup; live base replacement is refused"
            );
        }
        let previous = app.active_release.clone();
        let was_running = app.desired_running;
        self.engine
            .stop(&app.machine, app.spec.stop_timeout_seconds + 5, Some(log))?;
        self.write_launch(app, id)?;
        if !self.engine.exists(&app.machine)? {
            ensure!(
                util::hash_file(&dir.join("image.oci.tar"))? == release.image_sha256,
                "image archive corrupted"
            );
            self.engine.call(
                &Engine::create_args(app, &self.root, &dir.join("image.oci.tar"))?,
                300,
                Some(log),
            )?;
        }
        let attempt = (|| -> Result<()> {
            self.engine
                .machine("start", &app.machine, &[], 180, Some(log))?;
            self.wait_ready_release(app, id, log)
        })();
        if let Err(e) = attempt {
            let _ = self
                .engine
                .stop(&app.machine, app.spec.stop_timeout_seconds + 5, Some(log));
            if let Some(old) = &previous {
                self.write_launch(app, old)?;
                if was_running {
                    self.engine
                        .machine("start", &app.machine, &[], 180, Some(log))
                        .context("deployment failed and rollback start failed")?;
                    self.wait_ready_release(app, old, log)
                        .context("rollback failed readiness")?;
                }
            }
            return Err(e.context("deployment failed; previous release selected when available"));
        }
        if previous.as_deref() != Some(id) {
            app.previous_release = previous;
        }
        app.active_release = Some(id.into());
        app.desired_running = true;
        Ok(json!({"release":id,"running":true}))
    }
    fn wait_ready(&self, app: &Application, log: &Path) -> Result<()> {
        self.wait_ready_release(
            app,
            app.active_release.as_deref().context("no release")?,
            log,
        )
    }
    fn wait_ready_release(&self, app: &Application, id: &str, log: &Path) -> Result<()> {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let started = Instant::now();
        let mut last = "supervisor not ready".to_string();
        while started.elapsed() < Duration::from_secs(60) {
            match self.engine.exec(
                &app.machine,
                &[
                    crate::BUN.into(),
                    "/opt/defuss/supervisor.mjs".into(),
                    "probe".into(),
                ],
                10,
                None,
            ) {
                Ok(s) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&s) {
                        ensure!(
                            v["state"] != "exited",
                            "application exited during readiness: {v}"
                        );
                        if v["state"] == "running" && v["release"] == id {
                            if let Some(h) = &app.spec.health {
                                let port = app
                                    .spec
                                    .ports
                                    .iter()
                                    .find(|p| p.guest == h.port)
                                    .unwrap()
                                    .host;
                                match client
                                    .get(format!("http://127.0.0.1:{port}{}", h.path))
                                    .send()
                                {
                                    Ok(r) if r.status().is_success() => return Ok(()),
                                    Ok(r) => last = format!("health status {}", r.status()),
                                    Err(e) => last = e.to_string(),
                                }
                            } else if started.elapsed() >= Duration::from_secs(2) {
                                return Ok(());
                            }
                        } else {
                            last = format!("supervisor: {v}");
                        }
                    }
                }
                Err(e) => {
                    // VERIFIED: `--stop-on-exit` powers the machine off when the workload exits, so the probe can no
                    // longer reach the supervisor's status. A stopped machine cannot become ready: fail now
                    // instead of waiting out the timeout.
                    if !self.engine.is_running(&app.machine).unwrap_or(true) {
                        let logs = self
                            .engine
                            .machine(
                                "logs",
                                &app.machine,
                                &["--tail".into(), "100".into()],
                                15,
                                Some(log),
                            )
                            .unwrap_or_default();
                        anyhow::bail!(
                            "application exited during readiness; machine stopped: {logs}"
                        );
                    }
                    last = e.to_string();
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let logs = self
            .engine
            .machine(
                "logs",
                &app.machine,
                &["--tail".into(), "100".into()],
                15,
                Some(log),
            )
            .unwrap_or_default();
        anyhow::bail!("readiness timeout: {last}; {logs}")
    }
    fn maintenance(&self, app: &Application, log: &Path) -> Result<()> {
        ensure!(self.engine.exists(&app.machine)?, "no machine to maintain");
        self.engine
            .stop(&app.machine, app.spec.stop_timeout_seconds + 5, Some(log))?;
        self.engine.start_no_workload(&app.machine, 180, Some(log))
    }
    fn backup_current(&self, app: &Application, log: &Path) -> Result<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        self.engine.exec(
            &app.machine,
            &[
                crate::BUN.into(),
                "/opt/defuss/supervisor.mjs".into(),
                "backup".into(),
                id.clone(),
            ],
            600,
            Some(log),
        )?;
        let path = self
            .app_dir(&app.spec.name)
            .join("backups")
            .join(format!("{id}.tar"));
        let meta = json!({"id":id,"app":app.spec.name,"release":app.active_release,"created_at":util::now(),"sha256":util::hash_file(&path)?,"bytes":fs::metadata(&path)?.len(),"path":path});
        util::write_json(
            &self.root.join("backup-meta").join(format!("{id}.json")),
            &meta,
        )?;
        Ok(meta)
    }
    fn finish_maintenance(&self, app: &Application, log: &Path, restart: bool) -> Result<()> {
        self.engine.stop(&app.machine, 10, Some(log))?;
        if restart {
            self.engine
                .machine("start", &app.machine, &[], 180, Some(log))?;
            self.wait_ready(app, log)?;
        }
        Ok(())
    }
    fn backup(&self, app: &mut Application, log: &Path) -> Result<Value> {
        let restart = app.desired_running;
        self.maintenance(app, log)?;
        let result = self.backup_current(app, log);
        let resume = self.finish_maintenance(app, log, restart);
        let value = result?;
        resume?;
        Ok(value)
    }
    fn restore(&self, app: &mut Application, id: &str, log: &Path) -> Result<Value> {
        let meta: Value =
            util::read_json(&self.root.join("backup-meta").join(format!("{id}.json")))?;
        ensure!(
            meta["app"] == app.spec.name,
            "backup belongs to another app"
        );
        let hash = meta["sha256"].as_str().context("invalid backup metadata")?;
        let path = self
            .app_dir(&app.spec.name)
            .join("backups")
            .join(format!("{id}.tar"));
        ensure!(util::hash_file(&path)? == hash, "backup checksum mismatch");
        let restart = app.desired_running;
        self.maintenance(app, log)?;
        let safety = match self.backup_current(app, log) {
            Ok(value) => value,
            Err(e) => {
                self.finish_maintenance(app, log, restart)?;
                return Err(
                    e.context("pre-restore safety backup failed; restore was not attempted")
                );
            }
        };
        let result = self.engine.exec(
            &app.machine,
            &[
                crate::BUN.into(),
                "/opt/defuss/supervisor.mjs".into(),
                "restore".into(),
                id.into(),
                hash.into(),
            ],
            600,
            Some(log),
        );
        // Failed restore deliberately leaves application stopped; safety backup is retained.
        if let Err(e) = result {
            app.desired_running = false;
            self.store.save_app(app)?;
            self.engine.stop(&app.machine, 10, Some(log)).context("restore failed and maintenance machine could not stop; desired state persisted as stopped")?;
            return Err(e.context(format!(
                "restore failed; pre-restore backup {}",
                safety["id"]
            )));
        }
        self.finish_maintenance(app, log, restart)?;
        Ok(json!({"restored":id,"pre_restore_backup":safety}))
    }
    pub fn backups(&self, name: &str) -> Result<Vec<Value>> {
        let mut v = Vec::new();
        for e in fs::read_dir(self.root.join("backup-meta"))? {
            let p = e?.path();
            if p.extension().is_some_and(|e| e == "json") {
                let m: Value = util::read_json(&p)?;
                if m["app"] == name {
                    v.push(m);
                }
            }
        }
        Ok(v)
    }
    pub fn job_log(&self, id: &str) -> Result<String> {
        uuid::Uuid::parse_str(id)?;
        let p = self.root.join("jobs").join(format!("{id}.log"));
        Ok(fs::read_to_string(p).unwrap_or_default())
    }
}

/// Treat the builder as untrusted: only files inside this release's prefix may enter the image.
pub fn validate_app_layer(layer: &Path, id: &str) -> Result<()> {
    let prefix = format!("opt/defuss/releases/{id}/");
    let mut expanded = 0u64;
    let mut seen = std::collections::HashSet::new();
    let mut links = std::collections::HashSet::new();
    for entry in tar::Archive::new(fs::File::open(layer)?).entries()? {
        let e = entry?;
        let p = e.path()?.into_owned();
        let name = p.to_str().context("non UTF-8 archive path")?;
        ensure!(
            name.len() <= 4096 && seen.len() < 250_000,
            "archive path or entry-count limit exceeded"
        );
        ensure!(
            name.starts_with(&prefix)
                && !p.is_absolute()
                && !p
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                && !name.contains('\\'),
            "archive escapes release prefix"
        );
        ensure!(
            seen.insert(name.trim_end_matches('/').to_string()),
            "duplicate archive path"
        );
        for ancestor in p.ancestors().skip(1) {
            ensure!(
                !links.contains(&ancestor.to_string_lossy().into_owned()),
                "archive writes through symlink"
            );
        }
        let t = e.header().entry_type();
        ensure!(
            t.is_file() || t.is_dir() || t.is_symlink(),
            "unsupported layer entry"
        );
        expanded = expanded
            .checked_add(e.size())
            .context("layer size overflow")?;
        ensure!(expanded <= 8 * 1024 * 1024 * 1024, "layer exceeds 8 GiB");
        if t.is_symlink() {
            let target = e.link_name()?.context("missing symlink target")?;
            ensure!(
                !target.is_absolute()
                    && target.as_os_str().len() <= 4096
                    && !target.to_string_lossy().contains('\\'),
                "unsafe symlink refused"
            );
            let joined = p.parent().unwrap().join(target.as_ref());
            let mut depth = 0i32;
            for c in joined
                .strip_prefix(prefix.trim_end_matches('/'))?
                .components()
            {
                match c {
                    std::path::Component::ParentDir => depth -= 1,
                    std::path::Component::Normal(_) => depth += 1,
                    _ => {}
                }
                ensure!(depth >= 0, "escaping symlink");
            }
            links.insert(name.to_string());
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod lifecycle_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Harness {
        _temp: tempfile::TempDir,
        service: Service,
        app: Application,
        fake: PathBuf,
    }
    fn harness() -> Harness {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let fake = root.join("fake");
        fs::create_dir(&fake).unwrap();
        let script = fake.join("engine");
        fs::write(&script,r#"#!/usr/bin/env python3
import json,pathlib,sys
root=pathlib.Path(__file__).parent
args=sys.argv[1:]
with (root/'commands').open('a') as f:f.write(json.dumps(args)+'\n')
state=root/'state.json'
s=json.loads(state.read_text()) if state.exists() else {'exists':False,'running':False}
verb=args[1]
if verb=='ls':
 if s['exists']:print('defuss-test')
elif verb=='status':print(json.dumps({'state':'Running' if s['running'] else 'Stopped'}))
elif verb=='create':s['exists']=True
elif verb=='start':
 launch=json.loads((root.parent/'state/apps/hello/control/launch.json').read_text())
 crash=(root/'crash-release').read_text() if (root/'crash-release').exists() else ''
 # A crashing workload with --stop-on-exit powers the machine off right after start.
 s['running']='--no-workload' in args or launch['release']!=crash
 if '--no-workload' in args and (root/'exec-not-ready').exists():(root/'exec-pending').write_text((root/'exec-not-ready').read_text())
 # A broken provision-only boot: no exec succeeds until the machine stops (observed in smolvm 1.32.4).
 broken=int((root/'broken-boots').read_text()) if (root/'broken-boots').exists() else 0
 if '--no-workload' in args and broken:(root/'broken-boots').write_text(str(broken-1));(root/'exec-pending').write_text('1000000')
elif verb=='stop':
 s['running']=False
 if (root/'exec-pending').exists():(root/'exec-pending').unlink()
elif verb=='delete':s['exists']=False
elif verb=='logs':print('fixture logs')
elif verb=='exec':
 if not s['running']:sys.stderr.write('machine is not running\n');sys.exit(1)
 pending=int((root/'exec-pending').read_text()) if (root/'exec-pending').exists() else 0
 if pending:(root/'exec-pending').write_text(str(pending-1));sys.stderr.write('executable file `/usr/local/bin/bun` not found in $PATH\n');sys.exit(255)
 if args[-1]=='--version':print('1.4.4');state.write_text(json.dumps(s));sys.exit(0)
 launch=json.loads((root.parent/'state/apps/hello/control/launch.json').read_text())
 command=args[args.index('--')+1:]
 if command[-1]=='probe':
  fail=(root/'fail-release').read_text() if (root/'fail-release').exists() else ''
  print(json.dumps({'state':'exited' if launch['release']==fail else 'running','release':launch['release']}))
 elif command[2]=='backup':
  if (root/'fail-backup').exists():sys.exit(8)
  (root.parent/'state/apps/hello/backups'/ (command[3]+'.tar')).write_bytes(bytes(1024))
 elif command[2]=='restore' and (root/'fail-restore').exists():sys.exit(9)
state.write_text(json.dumps(s))
"#).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let service = Service::open(root.join("state"), script).unwrap();
        let spec:AppSpec=serde_json::from_value(json!({"name":"hello","repository":"https://github.com/example/app.git","mounts":root.join("shares")})).unwrap();
        let mut app = service.register(spec).unwrap();
        app.machine = "defuss-test".into();
        service.store.save_app(&app).unwrap();
        Harness {
            _temp: temp,
            service,
            app,
            fake,
        }
    }
    fn release(h: &Harness, base: &str) -> Release {
        let id = uuid::Uuid::new_v4().to_string();
        let dir = h.service.app_dir("hello").join("releases").join(&id);
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("app.tar"), vec![0u8; 1024]).unwrap();
        fs::write(dir.join("image.oci.tar"), vec![0u8; 1024]).unwrap();
        let r = Release {
            id,
            app: "hello".into(),
            commit: "a".repeat(40),
            base_digest: base.into(),
            architecture: "amd64".into(),
            layer_sha256: util::hash_file(&dir.join("app.tar")).unwrap(),
            image_sha256: util::hash_file(&dir.join("image.oci.tar")).unwrap(),
            created_at: 0,
        };
        h.service.store.add_release(&r).unwrap();
        r
    }
    #[test]
    fn deployment_switches_release_without_deleting_machine() {
        let mut h = harness();
        let a = release(&h, "base");
        let b = release(&h, "base");
        let log = h.fake.join("log");
        h.service.deploy(&mut h.app, &a.id, &log).unwrap();
        h.service.deploy(&mut h.app, &b.id, &log).unwrap();
        assert_eq!(h.app.active_release, Some(b.id));
        assert_eq!(h.app.previous_release, Some(a.id));
        let commands = fs::read_to_string(h.fake.join("commands")).unwrap();
        assert_eq!(
            commands
                .lines()
                .filter(|s| s.contains("\"create\""))
                .count(),
            1
        );
        assert!(!commands.contains("\"delete\""));
    }
    #[test]
    fn failed_deployment_restores_previous_release() {
        let mut h = harness();
        let a = release(&h, "base");
        let b = release(&h, "base");
        let log = h.fake.join("log");
        h.service.deploy(&mut h.app, &a.id, &log).unwrap();
        fs::write(h.fake.join("fail-release"), &b.id).unwrap();
        assert!(h.service.deploy(&mut h.app, &b.id, &log).is_err());
        assert_eq!(h.app.active_release, Some(a.id.clone()));
        let launch: Value =
            util::read_json(&h.service.app_dir("hello").join("control/launch.json")).unwrap();
        assert_eq!(launch["release"], a.id);
        assert!(h.service.engine.is_running("defuss-test").unwrap());
    }
    #[test]
    fn workload_that_stops_the_machine_fails_fast_and_restores_previous() {
        let mut h = harness();
        let a = release(&h, "base");
        let b = release(&h, "base");
        let log = h.fake.join("log");
        h.service.deploy(&mut h.app, &a.id, &log).unwrap();
        fs::write(h.fake.join("crash-release"), &b.id).unwrap();
        let started = Instant::now();
        let err = h.service.deploy(&mut h.app, &b.id, &log).unwrap_err();
        // Readiness gives up after 60 s; a stopped machine must not wait for that.
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "took {:?}",
            started.elapsed()
        );
        assert!(format!("{err:#}").contains("machine stopped"), "{err:#}");
        assert_eq!(h.app.active_release, Some(a.id));
        assert!(h.service.engine.is_running("defuss-test").unwrap());
    }
    #[test]
    fn backup_waits_until_a_provision_only_start_accepts_exec() {
        let mut h = harness();
        let a = release(&h, "base");
        let log = h.fake.join("log");
        h.service.deploy(&mut h.app, &a.id, &log).unwrap();
        // The first two execs after `start --no-workload` fail as the engine was observed to do once.
        fs::write(h.fake.join("exec-not-ready"), "2").unwrap();
        let backup = h.service.backup(&mut h.app, &log).unwrap();
        assert_eq!(h.service.backups("hello").unwrap()[0]["id"], backup["id"]);
        assert!(h.service.engine.is_running("defuss-test").unwrap());
    }
    #[test]
    fn backup_boots_again_when_a_provision_only_boot_never_accepts_exec() {
        let mut h = harness();
        h.service.engine.exec_wait = Duration::from_millis(300);
        let a = release(&h, "base");
        let log = h.fake.join("log");
        h.service.deploy(&mut h.app, &a.id, &log).unwrap();
        fs::write(h.fake.join("broken-boots"), "1").unwrap();
        let backup = h.service.backup(&mut h.app, &log).unwrap();
        assert_eq!(h.service.backups("hello").unwrap()[0]["id"], backup["id"]);
        let commands = fs::read_to_string(h.fake.join("commands")).unwrap();
        assert_eq!(commands.matches("--no-workload").count(), 2);
    }
    #[test]
    fn backup_fails_after_two_provision_only_boots_without_exec() {
        let mut h = harness();
        h.service.engine.exec_wait = Duration::from_millis(300);
        let a = release(&h, "base");
        let log = h.fake.join("log");
        h.service.deploy(&mut h.app, &a.id, &log).unwrap();
        fs::write(h.fake.join("broken-boots"), "2").unwrap();
        let error = h.service.backup(&mut h.app, &log).unwrap_err();
        assert!(
            format!("{error:#}").contains("never accepted exec"),
            "{error:#}"
        );
        assert!(h.service.backups("hello").unwrap().is_empty());
    }
    #[test]
    fn corrupted_release_and_base_change_are_rejected_before_stop() {
        let mut h = harness();
        let a = release(&h, "base");
        let b = release(&h, "different");
        h.app.active_release = Some(a.id);
        let log = h.fake.join("log");
        assert!(
            h.service
                .deploy(&mut h.app, &b.id, &log)
                .unwrap_err()
                .to_string()
                .contains("base image digest changed")
        );
        assert!(!h.fake.join("commands").exists());
        fs::write(
            h.service
                .app_dir("hello")
                .join("releases")
                .join(&b.id)
                .join("app.tar"),
            b"changed",
        )
        .unwrap();
        assert!(
            h.service
                .deploy(&mut h.app, &b.id, &log)
                .unwrap_err()
                .to_string()
                .contains("corrupted")
        );
    }
    #[test]
    fn failed_restore_keeps_workload_stopped_and_safety_backup() {
        let mut h = harness();
        let a = release(&h, "base");
        let log = h.fake.join("log");
        h.service.deploy(&mut h.app, &a.id, &log).unwrap();
        let backup = h.service.backup(&mut h.app, &log).unwrap();
        assert!(h.service.engine.is_running("defuss-test").unwrap());
        fs::write(h.fake.join("fail-restore"), b"yes").unwrap();
        let result = h
            .service
            .restore(&mut h.app, backup["id"].as_str().unwrap(), &log);
        assert!(result.is_err());
        assert!(!h.app.desired_running);
        assert!(!h.service.engine.is_running("defuss-test").unwrap());
        assert_eq!(h.service.backups("hello").unwrap().len(), 2);
    }
    #[test]
    fn safety_backup_failure_resumes_original_app_without_restoring() {
        let mut h = harness();
        let a = release(&h, "base");
        let log = h.fake.join("log");
        h.service.deploy(&mut h.app, &a.id, &log).unwrap();
        let backup = h.service.backup(&mut h.app, &log).unwrap();
        fs::write(h.fake.join("fail-backup"), b"yes").unwrap();
        assert!(
            h.service
                .restore(&mut h.app, backup["id"].as_str().unwrap(), &log)
                .is_err()
        );
        assert!(h.app.desired_running);
        assert!(h.service.engine.is_running("defuss-test").unwrap());
        assert!(
            !fs::read_to_string(h.fake.join("commands"))
                .unwrap()
                .contains("\"restore\"")
        );
    }
}
