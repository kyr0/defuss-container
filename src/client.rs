use crate::{model::Connection, util};
use anyhow::{Result, ensure};
use serde_json::Value;
use std::{path::Path, time::Duration};

pub struct Client {
    connection: Connection,
    http: reqwest::blocking::Client,
}
impl Client {
    pub fn connect(root: &Path) -> Result<Self> {
        let connection: Connection = util::read_json(&root.join("connection.json"))?;
        let url = url::Url::parse(&connection.address)?;
        ensure!(
            url.scheme() == "http"
                && url.host_str() == Some("127.0.0.1")
                && url.port().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path() == "/",
            "invalid local daemon address"
        );
        ensure!(
            connection.token.len() == 64 && connection.token.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid daemon token"
        );
        Ok(Self {
            connection,
            http: reqwest::blocking::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(45))
                .build()?,
        })
    }
    pub fn request(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        ensure!(
            matches!(method, "GET" | "POST")
                && path.starts_with("/v1/")
                && !path.contains(['?', '#', '\\'])
                && !path.split('/').any(|s| s == ".."),
            "invalid API request"
        );
        let mut r = self
            .http
            .request(
                method.parse()?,
                format!("{}{}", self.connection.address.trim_end_matches('/'), path),
            )
            .bearer_auth(&self.connection.token);
        if let Some(b) = body {
            r = r.json(&b)
        }
        let res = r.send()?;
        let status = res.status();
        let ct = res
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = res.text()?;
        ensure!(status.is_success(), "daemon {status}: {text}");
        Ok(if ct.contains("application/json") {
            serde_json::from_str(&text)?
        } else {
            Value::String(text)
        })
    }
}
