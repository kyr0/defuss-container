use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

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
        ensure!(
            (5..=240).contains(&self.stop_timeout_seconds),
            "stop timeout must be 5..240 seconds"
        );
        ensure!(
            self.ports.len() <= 16 && self.env.len() <= 128,
            "too many ports or environment variables"
        );
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
            ensure!(
                !k.starts_with("DEFUSS_")
                    && k != "BUN_OPTIONS"
                    && k != "NODE_OPTIONS"
                    && k != "LD_PRELOAD",
                "reserved environment key: {k}"
            );
            ensure!(
                v.len() <= 32768 && !v.contains('\0'),
                "invalid environment value"
            );
        }
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
