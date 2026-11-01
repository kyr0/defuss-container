//! Tauri plugin that forwards the desktop UI's API calls to the local daemon.
//!
//! The bridge exists so the bearer token stays in Rust: JavaScript sends a method, a path and a
//! body, and this side adds the token. It never starts or manages an engine or a daemon; the UI
//! shows recovery steps instead when the daemon is down.
use defuss_container::{client::Client, util};
use serde_json::Value;

/// Origin of the first page the main window loaded. defuss-tauri picks the loopback port at start,
/// so the origin is learned on the first navigation and every later navigation or command must
/// match it.
static UI_ORIGIN: std::sync::OnceLock<String> = std::sync::OnceLock::new();

#[tauri::command]
async fn daemon_request(
    window: tauri::WebviewWindow,
    method: String,
    path: String,
    body: Option<Value>,
) -> Result<Value, String> {
    // Custom app commands are allowed only from the packaged local UI, not navigation to a remote
    // site.
    let origin = window.url().map_err(|e| e.to_string())?;
    if UI_ORIGIN.get() != Some(&origin.origin().ascii_serialization()) {
        return Err("untrusted UI origin".into());
    }
    // The daemon API uses only GET and POST under /v1/ (docs/API.md); Client::request repeats this
    // check and also rejects queries, fragments and `..` segments.
    if !matches!(method.as_str(), "GET" | "POST") || !path.starts_with("/v1/") {
        return Err("invalid API route".into());
    }
    // Client is reqwest's blocking client, so it runs on a blocking thread, off Tauri's async
    // runtime. It reads connection.json on every call, which picks up the new port and token after
    // a daemon restart.
    tauri::async_runtime::spawn_blocking(move || {
        let client =
            Client::connect(&util::home()).map_err(|e| format!("Daemon unavailable: {e:#}"))?;
        client
            .request(&method, &path, body)
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| e.to_string())?
}
pub fn defuss_tauri_extend(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    // VERIFIED: defuss-tauri serves the UI from http://127.0.0.1:<port>, which Tauri 2.12.1 treats
    // as a remote origin; app commands from a remote origin are rejected ("not allowed by ACL")
    // unless a capability grants a permission. A plugin command has one
    // (`defuss-container:allow-daemon-request`), an app command has none.
    builder.plugin(
        tauri::plugin::Builder::<tauri::Wry>::new("defuss-container")
            .invoke_handler(tauri::generate_handler![daemon_request])
            // Only the packaged UI may load: loopback HTTP on the origin seen first. Navigation
            // anywhere else is refused, so no other page can load in this window and call
            // daemon_request.
            .on_navigation(|_, url| {
                if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") {
                    return false;
                }
                let origin = url.origin().ascii_serialization();
                UI_ORIGIN.get_or_init(|| origin.clone()) == &origin
            })
            .build(),
    )
}
