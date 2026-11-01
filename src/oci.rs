//! Restricted OCI builder: fetches the pinned `oven/bun:1.4.4-distroless` base and assembles one
//! image per release.
//!
//! Repository code runs inside the builder VM; this module only downloads, checks and copies blobs.
//! Every registry response is size-bounded and every blob is checked against its SHA-256 digest, so
//! a release records exactly which base it was built on (docs/SECURITY.md).
use crate::util;
use anyhow::{Context, Result, ensure};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const REGISTRY: &str = "https://registry-1.docker.io/v2/oven/bun";
/// Both index formats (for the tag) and both manifest formats (for the platform entry), whichever
/// the registry returns.
const ACCEPT: &str = concat!(
    "application/vnd.oci.image.index.v1+json, ",
    "application/vnd.docker.distribution.manifest.list.v2+json, ",
    "application/vnd.oci.image.manifest.v1+json, ",
    "application/vnd.docker.distribution.manifest.v2+json",
);
/// The resolved base image: its manifest digest (recorded in each release), its config and layer
/// descriptors, and the local cache directory that holds the verified layer blobs.
pub struct Base {
    pub digest: String,
    pub config: Value,
    pub layers: Vec<Value>,
    pub cache: PathBuf,
    pub architecture: String,
}
fn digest(v: &Value) -> Result<&str> {
    let s = v["digest"].as_str().context("missing digest")?;
    ensure!(
        s.starts_with("sha256:") && s.len() == 71 && s[7..].bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid sha256 digest"
    );
    Ok(s)
}
fn descriptor(kind: &str, bytes: &[u8]) -> Value {
    json!({"mediaType":kind,"digest":format!("sha256:{}",util::sha256(bytes)),"size":bytes.len()})
}
/// Reads a response body of at most `max` bytes, checking both the declared length and the bytes
/// actually read.
fn bounded(mut response: reqwest::blocking::Response, max: u64) -> Result<Vec<u8>> {
    response.error_for_status_ref()?;
    ensure!(
        response.content_length().unwrap_or(0) <= max,
        "registry response too large"
    );
    let mut b = Vec::new();
    (&mut response).take(max + 1).read_to_end(&mut b)?;
    ensure!(b.len() as u64 <= max, "registry response too large");
    Ok(b)
}
/// Returns the cached blob when its size and digest still match, else downloads it into a temporary
/// file and renames it into the cache only after the size and SHA-256 match the descriptor. A
/// partial or tampered download never lands under its digest name.
fn fetch_blob(client: &Client, token: &str, desc: &Value, cache: &Path) -> Result<PathBuf> {
    let d = digest(desc)?;
    let dest = cache.join(&d[7..]);
    let size = desc["size"].as_u64().context("missing blob size")?;
    ensure!(size <= 2 * 1024 * 1024 * 1024, "base blob exceeds 2 GiB");
    if dest.exists() && fs::metadata(&dest)?.len() == size && util::hash_file(&dest)? == d[7..] {
        return Ok(dest);
    }
    let mut res = client
        .get(format!("{REGISTRY}/blobs/{d}"))
        .bearer_auth(token)
        .send()?
        .error_for_status()?;
    let mut temp = tempfile::NamedTempFile::new_in(cache)?;
    let copied = std::io::copy(&mut (&mut res).take(size + 1), &mut temp)?;
    ensure!(
        copied == size && util::hash_file(temp.path())? == d[7..],
        "registry blob size or digest mismatch"
    );
    temp.as_file().sync_all()?;
    temp.persist(&dest).map_err(|e| e.error)?;
    Ok(dest)
}
/// Resolves the base for the host architecture: an anonymous pull token, the tag's index, the
/// linux/<arch> manifest (its digest checked), the config and every layer. The tag here must match
/// [`crate::BASE_IMAGE`].
pub fn pull(cache: &Path) -> Result<Base> {
    util::private_dir(cache)?;
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => anyhow::bail!("unsupported architecture"),
    }
    .to_string();
    let client = Client::builder()
        .timeout(Duration::from_secs(600))
        .build()?;
    let auth: Value = serde_json::from_slice(&bounded(
        client
            .get("https://auth.docker.io/token")
            .query(&[
                ("service", "registry.docker.io"),
                ("scope", "repository:oven/bun:pull"),
            ])
            .send()?,
        1024 * 1024,
    )?)?;
    let token = auth["token"].as_str().context("registry token absent")?;
    let mut bytes = bounded(
        client
            .get(format!("{REGISTRY}/manifests/1.4.4-distroless"))
            .header("Accept", ACCEPT)
            .bearer_auth(token)
            .send()?,
        8 * 1024 * 1024,
    )?;
    let mut manifest: Value = serde_json::from_slice(&bytes)?;
    if let Some(list) = manifest["manifests"].as_array() {
        let entry = list
            .iter()
            .find(|m| {
                m["platform"]["os"] == "linux" && m["platform"]["architecture"] == architecture
            })
            .context("Bun distroless image unavailable for this architecture")?;
        let d = digest(entry)?.to_string();
        bytes = bounded(
            client
                .get(format!("{REGISTRY}/manifests/{d}"))
                .header("Accept", ACCEPT)
                .bearer_auth(token)
                .send()?,
            8 * 1024 * 1024,
        )?;
        ensure!(
            format!("sha256:{}", util::sha256(&bytes)) == d,
            "manifest digest mismatch"
        );
        manifest = serde_json::from_slice(&bytes)?;
    }
    let base_digest = format!("sha256:{}", util::sha256(&bytes));
    let config =
        util::read_json::<Value>(&fetch_blob(&client, token, &manifest["config"], cache)?)?;
    ensure!(
        config["architecture"] == architecture && config["os"] == "linux",
        "base platform mismatch"
    );
    let layers = manifest["layers"]
        .as_array()
        .context("base layers absent")?
        .clone();
    ensure!(
        !layers.is_empty() && layers.len() <= 64,
        "invalid layer count"
    );
    for layer in &layers {
        fetch_blob(&client, token, layer, cache)?;
    }
    Ok(Base {
        digest: base_digest,
        config,
        layers,
        cache: cache.into(),
        architecture,
    })
}
// Tar entries get mtime 0 and uid/gid 0, so the same inputs always produce the same bytes and
// digests.
fn append_bytes(tar: &mut tar::Builder<File>, name: &str, data: &[u8], mode: u32) -> Result<()> {
    let mut h = tar::Header::new_gnu();
    h.set_size(data.len() as u64);
    h.set_mode(mode);
    h.set_mtime(0);
    h.set_uid(0);
    h.set_gid(0);
    h.set_cksum();
    tar.append_data(&mut h, name, data)?;
    Ok(())
}
fn append_file(tar: &mut tar::Builder<File>, name: &str, path: &Path) -> Result<()> {
    let mut h = tar::Header::new_gnu();
    h.set_size(fs::metadata(path)?.len());
    h.set_mode(0o644);
    h.set_mtime(0);
    h.set_uid(0);
    h.set_gid(0);
    h.set_cksum();
    tar.append_data(&mut h, name, File::open(path)?)?;
    Ok(())
}

/// Writes the release image: base layers, the app layer and a runtime layer with the guest
/// programs, as an OCI layout plus Docker's `manifest.json` for smolvm's importer. Returns the
/// SHA-256 of the image tar, which a deploy checks again before it creates a machine from it.
pub fn assemble(base: &Base, release: &str, layer: &Path, dest: &Path) -> Result<String> {
    let parent = dest.parent().context("image parent")?;
    let runtime = parent.join("runtime.tar");
    {
        let mut t = tar::Builder::new(File::create(&runtime)?);
        append_bytes(
            &mut t,
            "opt/defuss/supervisor.mjs",
            crate::SUPERVISOR.as_bytes(),
            0o644,
        )?;
        append_bytes(
            &mut t,
            "opt/defuss/archive.mjs",
            crate::ARCHIVE_JS.as_bytes(),
            0o644,
        )?;
        // VERIFIED: supervisor.mjs imports policy.mjs (packageScript); without it the workload
        // cannot start.
        append_bytes(
            &mut t,
            "opt/defuss/policy.mjs",
            crate::POLICY_JS.as_bytes(),
            0o644,
        )?;
        t.finish()?;
    }
    let mut layers = base.layers.clone();
    let mut config = base.config.clone();
    for p in [layer, runtime.as_path()] {
        let hash = util::hash_file(p)?;
        layers.push(json!({
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": format!("sha256:{hash}"),
            "size": fs::metadata(p)?.len(),
        }));
        config["rootfs"]["diff_ids"]
            .as_array_mut()
            .context("invalid base rootfs")?
            .push(json!(format!("sha256:{hash}")));
        if let Some(history) = config["history"].as_array_mut() {
            history.push(json!({"created_by":"defuss-container 0.1.0"}));
        }
    }
    config["config"]["Entrypoint"] = json!([crate::BUN, "/opt/defuss/supervisor.mjs"]);
    config["config"]["Cmd"] = json!([]);
    config["config"]["WorkingDir"] = json!("/app");
    config["config"]["User"] = json!("0:0");
    config["config"]["Labels"] = json!({
        "org.opencontainers.image.base.name": crate::BASE_IMAGE,
        "org.opencontainers.image.base.digest": base.digest,
        "org.defuss.release": release,
    });
    let cfg = serde_json::to_vec(&config)?;
    let cfg_desc = descriptor("application/vnd.oci.image.config.v1+json", &cfg);
    let manifest = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": cfg_desc,
        "layers": layers,
    }))?;
    let mut desc = descriptor("application/vnd.oci.image.manifest.v1+json", &manifest);
    desc["annotations"] =
        json!({"org.opencontainers.image.ref.name":format!("defuss/{release}:latest")});
    desc["platform"] = json!({"os":"linux","architecture":base.architecture});
    let index = serde_json::to_vec(&json!({"schemaVersion":2,"manifests":[desc]}))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut t = tar::Builder::new(temp.reopen()?);
        append_bytes(
            &mut t,
            "oci-layout",
            b"{\"imageLayoutVersion\":\"1.0.0\"}",
            0o644,
        )?;
        append_bytes(&mut t, "index.json", &index, 0o644)?;
        // smolvm's local importer uses Docker's manifest.json. Retain a complete OCI layout
        // and add this compatible index pointing to the exact same content-addressed blobs.
        let docker_layers: Vec<String> = layers
            .iter()
            .map(|l| {
                let digest = l["digest"].as_str().unwrap().trim_start_matches("sha256:");
                format!("blobs/sha256/{digest}")
            })
            .collect();
        let docker = serde_json::to_vec(&json!([{
            "Config": format!("blobs/sha256/{}", util::sha256(&cfg)),
            "RepoTags": [format!("defuss/{release}:latest")],
            "Layers": docker_layers,
        }]))?;
        append_bytes(&mut t, "manifest.json", &docker, 0o644)?;
        append_bytes(
            &mut t,
            &format!("blobs/sha256/{}", util::sha256(&cfg)),
            &cfg,
            0o644,
        )?;
        append_bytes(
            &mut t,
            &format!("blobs/sha256/{}", util::sha256(&manifest)),
            &manifest,
            0o644,
        )?;
        let mut seen = std::collections::HashSet::new();
        for l in &base.layers {
            let d = &digest(l)?[7..];
            if seen.insert(d.to_string()) {
                append_file(&mut t, &format!("blobs/sha256/{d}"), &base.cache.join(d))?;
            }
        }
        for p in [layer, runtime.as_path()] {
            let d = util::hash_file(p)?;
            if seen.insert(d.clone()) {
                append_file(&mut t, &format!("blobs/sha256/{d}"), p)?;
            }
        }
        t.finish()?;
    }
    temp.flush()?;
    temp.as_file().sync_all()?;
    let hash = util::hash_file(temp.path())?;
    temp.persist(dest).map_err(|e| e.error)?;
    Ok(hash)
}
