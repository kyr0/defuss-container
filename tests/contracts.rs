use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use defuss_container::{
    api::{ApiState, router},
    model::*,
    oci::{self, Base},
    provider::Engine,
    service::{Service, validate_app_layer},
    store::Store,
    util,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use tower::ServiceExt;
fn spec(root: &Path) -> AppSpec {
    serde_json::from_value(json!({"name":"hello","repository":"https://github.com/example/app.git","mounts":dunce::canonicalize(root).unwrap().join("shares"),"ports":[{"host":8080,"guest":3000}],"health":{"port":3000,"path":"/health"}})).unwrap()
}
fn app(root: &Path) -> Application {
    Application {
        spec: spec(root),
        machine: "defuss-test".into(),
        active_release: None,
        previous_release: None,
        desired_running: false,
        last_error: None,
    }
}
#[test]
fn spec_accepts_contract() {
    let t = tempfile::tempdir().unwrap();
    spec(t.path()).validate().unwrap();
}
#[test]
fn spec_rejects_remote_command_and_path_injection() {
    let t = tempfile::tempdir().unwrap();
    for name in ["../x", "x;id", "-x", "A", "a/b"] {
        let mut s = spec(t.path());
        s.name = name.into();
        assert!(s.validate().is_err());
    }
    for u in [
        "file:///etc",
        "ssh://host/repo",
        "https://user:pass@host/repo",
        "https://host/repo?token=x",
    ] {
        let mut s = spec(t.path());
        s.repository = u.into();
        assert!(s.validate().is_err());
    }
    for p in ["../secret", "a/../../b", "/root", "a\\b"] {
        let mut s = spec(t.path());
        s.subdirectory = p.into();
        assert!(s.validate().is_err());
    }
}
#[test]
fn spec_rejects_invalid_resources_env_and_health() {
    let t = tempfile::tempdir().unwrap();
    let mut s = spec(t.path());
    s.cpus = 0;
    assert!(s.validate().is_err());
    s.cpus = 2;
    s.env.insert("DEFUSS_VAR".into(), "/secret".into());
    assert!(s.validate().is_err());
    s.env.clear();
    s.ports.push(Port {
        host: 8080,
        guest: 2,
    });
    assert!(s.validate().is_err());
    s.ports.pop();
    s.health.as_mut().unwrap().port = 999;
    assert!(s.validate().is_err());
}
#[test]
fn destructive_confirmation_and_ids() {
    assert!(
        Action::Delete {
            confirm: "no".into()
        }
        .validate("hello")
        .is_err()
    );
    assert!(
        Action::Delete {
            confirm: "hello".into()
        }
        .validate("hello")
        .is_ok()
    );
    assert!(
        Action::Deploy {
            release: "../../etc".into()
        }
        .validate("hello")
        .is_err()
    );
}
#[test]
fn sqlite_recovers_interrupted_job_and_serializes_app_jobs() {
    let t = tempfile::tempdir().unwrap();
    let db = Store::open(t.path()).unwrap();
    db.insert_app(&app(t.path())).unwrap();
    let job = Job {
        id: "a".into(),
        app: "hello".into(),
        action: Action::Build,
        state: "queued".into(),
        created_at: 0,
        completed_at: None,
        result: None,
        error: None,
    };
    db.insert_job(&job).unwrap();
    let mut duplicate = job.clone();
    duplicate.id = "b".into();
    assert!(db.insert_job(&duplicate).is_err());
    db.recover_jobs().unwrap();
    assert_eq!(db.job("a").unwrap().state, "interrupted");
    db.insert_job(&duplicate).unwrap();
    drop(db);
    assert_eq!(
        Store::open(t.path()).unwrap().app("hello").unwrap().machine,
        "defuss-test"
    );
}
#[test]
fn generated_machine_uses_durable_storage_and_no_shell() {
    let t = tempfile::tempdir().unwrap();
    // The daemon canonicalizes its state root at startup; macOS temp dirs sit below the /var symlink.
    let root = dunce::canonicalize(t.path()).unwrap();
    let a = app(&root);
    let args = Engine::create_args(&a, &root, &root.join("space directory/image.tar")).unwrap();
    assert!(args.windows(2).any(|a| a == ["--disk-durability", "full"]));
    assert!(args.windows(2).any(|a| a == ["--volume", "/app/var"]));
    assert!(
        args.windows(2)
            .any(|a| a == ["--guest-subnet", "100.96.0.0/30"])
    );
    assert_eq!(args.last().unwrap(), "/opt/defuss/supervisor.mjs");
    assert!(!args.iter().any(|a| a == "sh"));
}
#[cfg(unix)]
#[test]
fn rejects_symlink_mounts() {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir(t.path().join("shares")).unwrap();
    std::os::unix::fs::symlink("/tmp", t.path().join("shares/input")).unwrap();
    assert!(Engine::create_args(&app(t.path()), t.path(), &t.path().join("image.tar")).is_err());
}
fn layer(path: &Path, entries: &[(&str, Option<&str>)]) {
    let mut t = tar::Builder::new(fs::File::create(path).unwrap());
    for (name, link) in entries {
        let mut h = tar::Header::new_gnu();
        h.set_mode(0o644);
        h.set_size(0);
        if let Some(target) = link {
            h.set_entry_type(tar::EntryType::Symlink);
            h.set_link_name(target).unwrap();
        }
        h.set_cksum();
        t.append_data(&mut h, name, std::io::empty()).unwrap();
    }
    t.finish().unwrap();
}
#[test]
fn layer_refuses_outside_prefix() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("app.tar");
    layer(&p, &[("etc/passwd", None)]);
    assert!(validate_app_layer(&p, "test").is_err());
}
#[test]
fn layer_refuses_absolute_or_escaping_links() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("app.tar");
    for target in ["/etc/passwd", "../../../../../secret"] {
        layer(&p, &[("opt/defuss/releases/test/link", Some(target))]);
        assert!(validate_app_layer(&p, "test").is_err());
    }
}
#[test]
fn layer_refuses_symlink_parent_and_duplicates() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("app.tar");
    layer(
        &p,
        &[
            ("opt/defuss/releases/test/link", Some("ok")),
            ("opt/defuss/releases/test/link/child", None),
        ],
    );
    assert!(validate_app_layer(&p, "test").is_err());
    layer(
        &p,
        &[
            ("opt/defuss/releases/test/x", None),
            ("opt/defuss/releases/test/x", None),
        ],
    );
    assert!(validate_app_layer(&p, "test").is_err());
}
#[test]
fn layer_accepts_node_bin_symlink() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("app.tar");
    layer(
        &p,
        &[
            (
                "opt/defuss/releases/test/node_modules/.bin/pkg",
                Some("../pkg/cli.js"),
            ),
            ("opt/defuss/releases/test/package.json", None),
        ],
    );
    validate_app_layer(&p, "test").unwrap();
}
#[test]
fn oci_layout_digests_config_and_docker_compatibility() {
    let t = tempfile::tempdir().unwrap();
    let cache = t.path().join("cache");
    fs::create_dir(&cache).unwrap();
    let base_bytes = b"empty base test bytes";
    let d = util::sha256(base_bytes);
    fs::write(cache.join(&d), base_bytes).unwrap();
    let base = Base {
        digest: format!("sha256:{d}"),
        architecture: "amd64".into(),
        cache,
        config: json!({"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[format!("sha256:{d}")]},"config":{},"history":[]}),
        layers: vec![
            json!({"mediaType":"application/vnd.oci.image.layer.v1.tar","digest":format!("sha256:{d}"),"size":base_bytes.len()}),
        ],
    };
    let app = t.path().join("app.tar");
    layer(&app, &[("opt/defuss/releases/test/package.json", None)]);
    let out = t.path().join("image.tar");
    let hash = oci::assemble(&base, "test", &app, &out).unwrap();
    assert_eq!(hash, util::hash_file(&out).unwrap());
    let mut blobs = std::collections::HashMap::new();
    for e in tar::Archive::new(fs::File::open(out).unwrap())
        .entries()
        .unwrap()
    {
        let mut e = e.unwrap();
        let name = e.path().unwrap().to_string_lossy().into_owned();
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut e, &mut b).unwrap();
        if let Some(h) = name.strip_prefix("blobs/sha256/") {
            assert_eq!(h, util::sha256(&b));
        }
        blobs.insert(name, b);
    }
    let index: Value = serde_json::from_slice(&blobs["index.json"]).unwrap();
    let manifest: Value = serde_json::from_slice(
        &blobs[&format!(
            "blobs/sha256/{}",
            index["manifests"][0]["digest"]
                .as_str()
                .unwrap()
                .strip_prefix("sha256:")
                .unwrap()
        )],
    )
    .unwrap();
    assert_eq!(manifest["layers"].as_array().unwrap().len(), 3);
    let docker: Value = serde_json::from_slice(&blobs["manifest.json"]).unwrap();
    let cfg: Value = serde_json::from_slice(&blobs[docker[0]["Config"].as_str().unwrap()]).unwrap();
    assert_eq!(cfg["config"]["Entrypoint"][0], defuss_container::BUN);
    assert_eq!(cfg["rootfs"]["diff_ids"].as_array().unwrap().len(), 3);
}
fn test_api() -> (tempfile::TempDir, axum::Router) {
    let t = tempfile::tempdir().unwrap();
    let service = Arc::new(Service::open(t.path().join("state"), "missing-smolvm".into()).unwrap());
    let r = router(ApiState {
        service,
        token: "a".repeat(64),
        authority: "127.0.0.1:12345".into(),
    });
    (t, r)
}
fn request(path: &str) -> axum::http::request::Builder {
    Request::builder()
        .uri(path)
        .header("Host", "127.0.0.1:12345")
        .header("Authorization", format!("Bearer {}", "a".repeat(64)))
}
#[tokio::test]
async fn api_refuses_missing_or_wrong_token() {
    let (_t, r) = test_api();
    for token in [None, Some("Bearer nope")] {
        let mut q = Request::builder()
            .uri("/v1/apps")
            .header("Host", "127.0.0.1:12345");
        if let Some(t) = token {
            q = q.header("Authorization", t)
        }
        assert_eq!(
            r.clone()
                .oneshot(q.body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}
#[tokio::test]
async fn api_rejects_browser_origin_and_dns_rebinding() {
    let (_t, r) = test_api();
    let q = request("/v1/apps").header("Origin", "https://evil.example");
    assert_eq!(
        r.clone()
            .oneshot(q.body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let q = Request::builder()
        .uri("/v1/apps")
        .header("Host", "evil.example:12345")
        .header("Authorization", format!("Bearer {}", "a".repeat(64)));
    assert_eq!(
        r.oneshot(q.body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn api_create_list_duplicate_and_unknown_fields() {
    let (t, r) = test_api();
    let data = serde_json::to_vec(&spec(t.path())).unwrap();
    let make = || {
        request("/v1/apps")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(data.clone()))
            .unwrap()
    };
    assert_eq!(
        r.clone().oneshot(make()).await.unwrap().status(),
        StatusCode::CREATED
    );
    assert_eq!(
        r.clone().oneshot(make()).await.unwrap().status(),
        StatusCode::CONFLICT
    );
    let res = r
        .oneshot(request("/v1/apps").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.headers()["cache-control"], "no-store");
    let v: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(v[0]["spec"]["name"], "hello");
}
#[tokio::test]
async fn api_malformed_action_is_rejected() {
    let (t, r) = test_api();
    let data = serde_json::to_vec(&spec(t.path())).unwrap();
    r.clone()
        .oneshot(
            request("/v1/apps")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(data))
                .unwrap(),
        )
        .await
        .unwrap();
    let response = r
        .oneshot(
            request("/v1/apps/hello/actions")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action":"delete","confirm":"wrong"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
#[tokio::test]
async fn failed_job_is_persisted_without_engine() {
    let (t, r) = test_api();
    let data = serde_json::to_vec(&spec(t.path())).unwrap();
    r.clone()
        .oneshot(
            request("/v1/apps")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(data))
                .unwrap(),
        )
        .await
        .unwrap();
    let response = r
        .clone()
        .oneshot(
            request("/v1/apps/hello/actions")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"action":"build"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let v: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let path = format!("/v1/jobs/{}", v["id"].as_str().unwrap());
    for _ in 0..100 {
        let res = r
            .clone()
            .oneshot(request(&path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let job: Job =
            serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
        if job.state == "failed" {
            assert!(job.error.unwrap().contains("launch child process"));
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("job did not finish");
}
#[cfg(unix)]
#[test]
fn process_timeout_kills_process_group() {
    let result = defuss_container::process::run(
        "sh",
        &["-c".into(), "sleep 20 & wait".into()],
        None,
        std::time::Duration::from_millis(100),
        None,
    );
    assert!(result.err().unwrap().to_string().contains("timed out"));
}
#[test]
fn client_rejects_remote_discovery_address() {
    let t = tempfile::tempdir().unwrap();
    util::write_json(
        &t.path().join("connection.json"),
        &json!({"address":"http://example.com:8080","token":"a".repeat(64)}),
    )
    .unwrap();
    assert!(defuss_container::client::Client::connect(t.path()).is_err());
}
