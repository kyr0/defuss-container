//! API and storage types with their validation.
//!
//! Specs and actions are validated where they enter the daemon, so every stored record is valid.
//! Every request type uses `deny_unknown_fields`, which turns a misspelt field into an error
//! instead of a silent default.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

/// What the user registers: where the code comes from and how its VM is sized, shared and checked.
/// `mounts` is the host directory whose `input`, `output` and `etc` subdirectories the VM sees;
/// ports publish guest ports on the host; `health` is the HTTP readiness check a deploy waits for.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSpec {
    pub name: String,
    pub repository: String,
    #[serde(default = "default_ref")]
    pub git_ref: String,
    #[serde(default)]
    pub subdirectory: String,
    pub mounts: PathBuf,
    #[serde(default = "default_cpus")]
    pub cpus: u8,
    #[serde(default = "default_memory")]
    pub memory_mib: u32,
    #[serde(default = "default_storage")]
    pub storage_gib: u64,
    #[serde(default)]
    pub ports: Vec<Port>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub build: bool,
    #[serde(default)]
    pub health: Option<Health>,
    #[serde(default = "default_stop")]
    pub stop_timeout_seconds: u64,
}
fn default_ref() -> String {
    "HEAD".into()
}
fn default_cpus() -> u8 {
    2
}
fn default_memory() -> u32 {
    1024
}
fn default_storage() -> u64 {
    20
}
fn default_stop() -> u64 {
    30
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Port {
    pub host: u16,
    pub guest: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Health {
    pub port: u16,
    pub path: String,
}

/// The name becomes a directory under `apps/` and a segment of `/v1/apps/{name}`, so it is limited
/// to lowercase letters, digits and `-`, starting with a letter. The VM itself gets a random
/// `defuss-<uuid>` machine name.
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 48
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl AppSpec {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            valid_name(&self.name),
            "name must match [a-z][a-z0-9-]{{0,47}}"
        );
        // Git runs on the host, so only HTTPS remotes pass: a file: URL would read host paths and
        // an SSH remote would use the user's SSH keys. Credentials in the URL would be stored in
        // plain text with the spec.
        let u = url::Url::parse(&self.repository)?;
        ensure!(
            u.scheme() == "https" && u.host_str().is_some(),
            "repository must be an HTTPS Git URL"
        );
        ensure!(
            u.username().is_empty()
                && u.password().is_none()
                && u.query().is_none()
                && u.fragment().is_none(),
            "repository credentials, query and fragment are forbidden; use Git credential configuration"
        );
        ensure!(
            !self.git_ref.is_empty()
                && self.git_ref.len() <= 256
                // A leading `-` would make Git parse the ref as an option.
                && !self.git_ref.starts_with('-')
                && !self.git_ref.chars().any(char::is_whitespace),
            "invalid git_ref"
        );
        ensure!(
            self.subdirectory
                .split('/')
                .all(|s| s != ".." && !s.contains('\\'))
                && !self.subdirectory.starts_with('/'),
            "subdirectory must be relative without traversal"
        );
        ensure!(
            self.mounts.is_absolute(),
            "mounts must be an absolute host directory"
        );
        ensure!(
            (1..=64).contains(&self.cpus)
                && (256..=262144).contains(&self.memory_mib)
                && (2..=1024).contains(&self.storage_gib),
            "resource limits outside supported range"
        );
        // VERIFIED: smolvm caps `--stop-timeout` at 300 s (docs/VERIFICATION.md, Engine CLI
        // contract); the daemon passes this value + 5, which stays below it.
        ensure!(
            (5..=240).contains(&self.stop_timeout_seconds),
            "stop timeout must be 5..240 seconds"
        );
        ensure!(
            self.ports.len() <= 16 && self.env.len() <= 128,
            "too many ports or environment variables"
        );
        // Host ports below 1024 need root on Linux and macOS, and the daemon runs as the user.
        let mut used = std::collections::BTreeSet::new();
        for p in &self.ports {
            ensure!(
                p.host >= 1024 && p.guest > 0 && used.insert(p.host),
                "ports must be nonzero, unique and host ports >=1024"
            );
        }
        for (k, v) in &self.env {
            ensure!(
                !k.is_empty()
                    && k.len() <= 128
                    && !k.as_bytes()[0].is_ascii_digit()
                    && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                "invalid environment key"
            );
            // The supervisor sets the DEFUSS_* variables after the app's env
            // (guest/supervisor.mjs), so an app value would be silently overwritten. Every other
            // key, BUN_OPTIONS and LD_PRELOAD included, reaches only the app's own processes in its
            // VM: the env is written to launch.json and never applied on the host. VERIFIED: the
            // e2e deploys with BUN_OPTIONS and the app reports the flag in process.execArgv.
            ensure!(!k.starts_with("DEFUSS_"), "reserved environment key: {k}");
            ensure!(
                v.len() <= 32768 && !v.contains('\0'),
                "invalid environment value"
            );
        }
        // The daemon probes `http://127.0.0.1:<host port><path>` (service.rs wait_ready), so the
        // path must be a plain absolute path without CR/LF, and the health port must be one of the
        // published guest ports.
        if let Some(h) = &self.health {
            ensure!(
                h.path.starts_with('/')
                    && !h.path.starts_with("//")
                    && !h.path.contains(['\r', '\n'])
                    && h.path.len() <= 1024,
                "invalid health path"
            );
            ensure!(
                self.ports.iter().any(|p| p.guest == h.port),
                "health port must be published"
            );
        }
        Ok(())
    }
}

/// A finished build. Saved only after its artifacts are complete, so every stored release is
/// deployable. The checksums are checked again before a deploy, and `base_digest` pins the base
/// image a machine was created from.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Release {
    pub id: String,
    pub app: String,
    pub commit: String,
    pub base_digest: String,
    pub architecture: String,
    pub layer_sha256: String,
    pub image_sha256: String,
    pub created_at: u64,
}

/// A registered app. `desired_running` is the state the daemon restores after a restart;
/// `previous_release` is what rollback and a failed deploy switch back to.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Application {
    pub spec: AppSpec,
    pub machine: String,
    pub active_release: Option<String>,
    pub previous_release: Option<String>,
    pub desired_running: bool,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "action", rename_all = "snake_case")]
pub enum Action {
    Build,
    Deploy { release: String },
    Start,
    Stop,
    Rollback,
    Backup,
    Restore { backup: String },
    Delete { confirm: String },
}

impl Action {
    pub fn validate(&self, name: &str) -> Result<()> {
        match self {
            Self::Deploy { release } | Self::Restore { backup: release } => {
                uuid::Uuid::parse_str(release)?;
            }
            Self::Delete { confirm } if confirm != name => {
                bail!("delete confirmation must equal application name")
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub app: String,
    pub action: Action,
    pub state: String,
    pub created_at: u64,
    pub completed_at: Option<u64>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Connection {
    pub address: String,
    pub token: String,
}
