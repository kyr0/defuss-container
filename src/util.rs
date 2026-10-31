use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn home() -> PathBuf {
    std::env::var_os("DEFUSS_CONTAINER_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_local_dir()
                .expect("OS data directory")
                .join("defuss-container")
        })
}
pub fn private_dir(p: &Path) -> Result<()> {
    fs::create_dir_all(p)?;
    ensure!(
        !fs::symlink_metadata(p)?.file_type().is_symlink(),
        "symlink directory refused: {}",
        p.display()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub fn atomic_write(p: &Path, data: &[u8]) -> Result<()> {
    let parent = p.parent().context("missing parent")?;
    fs::create_dir_all(parent)?;
    let mut f = tempfile::NamedTempFile::new_in(parent)?;
    f.write_all(data)?;
    f.as_file().sync_all()?;
    f.persist(p).map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn write_json<T: serde::Serialize>(p: &Path, v: &T) -> Result<()> {
    atomic_write(p, &serde_json::to_vec_pretty(v)?)
}
pub fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Result<T> {
    Ok(serde_json::from_slice(
        &fs::read(p).with_context(|| format!("read {}", p.display()))?,
    )?)
}
pub fn sha256(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}
pub fn hash_file(p: &Path) -> Result<String> {
    let mut f = File::open(p)?;
    let mut h = Sha256::new();
    let mut b = [0u8; 65536];
    loop {
        let n = f.read(&mut b)?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(hex::encode(h.finalize()))
}
pub fn checked_child(root: &Path, id: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(id)?;
    Ok(root.join(id))
}
pub fn ensure_no_symlinks(path: &Path) -> Result<()> {
    let mut p = PathBuf::new();
    for c in path.components() {
        p.push(c);
        if p.exists() {
            ensure!(
                !fs::symlink_metadata(&p)?.file_type().is_symlink(),
                "symlink path refused: {}",
                p.display()
            );
        }
    }
    Ok(())
}
