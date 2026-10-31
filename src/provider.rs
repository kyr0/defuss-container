use crate::{ENGINE_VERSION, model::Application, process, util};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub trait Platform: Send + Sync {
    fn name(&self) -> &'static str;
    fn prerequisite(&self) -> Result<()>;
}
pub struct Linux;
pub struct MacOs;
pub struct Windows;
impl Platform for Linux {
    fn name(&self) -> &'static str {
        "linux-kvm"
    }
    fn prerequisite(&self) -> Result<()> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
            .context(
                "KVM unavailable: enable virtualization and grant this user /dev/kvm access",
            )?;
        Ok(())
    }
}
impl Platform for MacOs {
    fn name(&self) -> &'static str {
        "macos-hvf"
    }
    fn prerequisite(&self) -> Result<()> {
        ensure!(
            std::env::consts::ARCH == "aarch64",
            "macOS requires Apple Silicon"
        );
        let s = process::checked(
            "sysctl",
            &["-n".into(), "kern.hv_support".into()],
            None,
            Duration::from_secs(5),
            None,
        )?;
        ensure!(s.trim() == "1", "Hypervisor.framework is unavailable");
        Ok(())
    }
}
impl Platform for Windows {
    fn name(&self) -> &'static str {
        "windows-whp"
    }
    fn prerequisite(&self) -> Result<()> {
        ensure!(
            std::env::consts::ARCH == "x86_64",
            "Windows requires x86_64"
        );
        let s = process::checked(
            "powershell.exe",
            &[
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                "(Get-CimInstance Win32_ComputerSystem).HypervisorPresent".into(),
            ],
            None,
            Duration::from_secs(20),
            None,
        )?;
        ensure!(
            s.trim().eq_ignore_ascii_case("true"),
            "Enable Windows Hypervisor Platform and hardware virtualization, then reboot"
        );
        Ok(())
    }
}
#[derive(Clone)]
pub struct Engine {
    pub executable: PathBuf,
}
impl Engine {
    pub fn platform(&self) -> Box<dyn Platform> {
        #[cfg(target_os = "linux")]
        {
            Box::new(Linux)
        }
        #[cfg(target_os = "macos")]
        {
            Box::new(MacOs)
        }
        #[cfg(target_os = "windows")]
        {
            Box::new(Windows)
        }
    }
    pub fn verify(&self) -> Result<()> {
        let v = self.call(&["--version".into()], 30, None)?;
        ensure!(
            v.split_whitespace().any(|s| s == ENGINE_VERSION),
            "smolvm {ENGINE_VERSION} required; engine reported {}",
            v.trim()
        );
        self.platform().prerequisite()?;
        Ok(())
    }
    pub fn doctor(&self) -> Value {
        let version = self.call(&["--version".into()], 30, None);
        let prerequisite = self.platform().prerequisite();
        json!({"provider":self.platform().name(),"architecture":std::env::consts::ARCH,"expected_engine":ENGINE_VERSION,
          "engine":self.executable,"version":version.as_ref().ok().map(|s|s.trim()),"engine_error":version.err().map(|e|e.to_string()),
          "hypervisor_ready":prerequisite.is_ok(),"hypervisor_error":prerequisite.err().map(|e|e.to_string()),
          "note":"Prerequisite checks do not prove VM boot or volume correctness. Run make e2e (tests/e2e/run.py)."})
    }
    pub fn call(&self, args: &[String], seconds: u64, log: Option<&Path>) -> Result<String> {
        process::checked(
            &self.executable,
            args,
            None,
            Duration::from_secs(seconds),
            log,
        )
    }
    pub fn machine(
        &self,
        verb: &str,
        name: &str,
        extra: &[String],
        seconds: u64,
        log: Option<&Path>,
    ) -> Result<String> {
        let mut a = vec!["machine".into(), verb.into(), "--name".into(), name.into()];
        a.extend_from_slice(extra);
        self.call(&a, seconds, log)
    }
    pub fn state(&self, name: &str) -> Result<Value> {
        Ok(serde_json::from_str(&self.machine(
            "status",
            name,
            &["--json".into()],
            30,
            None,
        )?)?)
    }
    pub fn exists(&self, name: &str) -> Result<bool> {
        let s = self.call(&["machine".into(), "ls".into(), "--quiet".into()], 30, None)?;
        Ok(s.lines().any(|s| s.trim() == name))
    }
    pub fn is_running(&self, name: &str) -> Result<bool> {
        Ok(self.state(name)?["state"]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("running")))
    }
    pub fn stop(&self, name: &str, seconds: u64, log: Option<&Path>) -> Result<()> {
        if self.exists(name)? && self.is_running(name)? {
            self.machine(
                "stop",
                name,
                &["--timeout".into(), seconds.to_string()],
                seconds + 60,
                log,
            )?;
        }
        Ok(())
    }
    pub fn delete(&self, name: &str, log: Option<&Path>) -> Result<()> {
        if self.exists(name)? {
            self.machine("delete", name, &["--force".into()], 120, log)?;
        }
        Ok(())
    }
    pub fn exec(
        &self,
        name: &str,
        cmd: &[String],
        seconds: u64,
        log: Option<&Path>,
    ) -> Result<String> {
        let mut a = vec!["--".into()];
        a.extend_from_slice(cmd);
        self.machine("exec", name, &a, seconds, log)
    }
    /// Boots the machine without its workload and returns once its container accepts exec.
    ///
    /// VERIFIED: once in about 30 maintenance starts (macOS, 2026-10-31) an exec issued right after smolvm 1.32.4
    /// reported the provision-only start failed with "executable file `/usr/local/bin/bun` not found"; a later exec
    /// works. UNKNOWN: the engine-side cause. Waiting on `bun --version` turns that race into a bounded delay.
    pub fn start_no_workload(&self, name: &str, seconds: u64, log: Option<&Path>) -> Result<()> {
        self.machine("start", name, &["--no-workload".into()], seconds, log)?;
        let started = std::time::Instant::now();
        loop {
            match self.exec(name, &[crate::BUN.into(), "--version".into()], 30, None) {
                Ok(_) => return Ok(()),
                Err(e) if started.elapsed() >= Duration::from_secs(30) => {
                    return Err(e.context("machine started but its container never accepted exec"));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(250)),
            }
        }
    }
    pub fn create_args(app: &Application, root: &Path, image: &Path) -> Result<Vec<String>> {
        let dir = root.join("apps").join(&app.spec.name);
        let mut a = vec![
            "machine".into(),
            "create".into(),
            "--name".into(),
            app.machine.clone(),
            "--image".into(),
            image.to_string_lossy().into_owned(),
            "--net".into(),
            "--net-backend".into(),
            "virtio-net".into(),
            "--guest-subnet".into(),
            "100.96.0.0/30".into(),
            "--cpus".into(),
            app.spec.cpus.to_string(),
            "--mem".into(),
            app.spec.memory_mib.to_string(),
            "--storage".into(),
            app.spec.storage_gib.to_string(),
            "--disk-durability".into(),
            "full".into(),
            "--user".into(),
            "0:0".into(),
            "--stop-timeout".into(),
            (app.spec.stop_timeout_seconds + 5).to_string(),
            "--stop-on-exit".into(),
            "--label".into(),
            "managed-by=defuss-container".into(),
            "--volume".into(),
            "/app/var".into(),
            "--volume".into(),
            "/app/tmp".into(),
        ];
        for (host, guest, mode) in [
            (dir.join("control"), "/defuss-control", "ro"),
            (dir.join("releases"), "/defuss-releases", "ro"),
            (dir.join("backups"), "/defuss-backups", "rw"),
            (app.spec.mounts.join("input"), "/app/input", "ro"),
            (app.spec.mounts.join("output"), "/app/output", "rw"),
            (app.spec.mounts.join("etc"), "/app/etc", "ro"),
        ] {
            util::ensure_no_symlinks(&host)?;
            a.extend([
                "--volume".into(),
                format!("{}:{guest}:{mode}", host.display()),
            ]);
        }
        for p in &app.spec.ports {
            a.extend(["--port".into(), format!("{}:{}", p.host, p.guest)]);
        }
        a.extend([
            "--".into(),
            crate::BUN.into(),
            "/opt/defuss/supervisor.mjs".into(),
        ]);
        Ok(a)
    }
}
