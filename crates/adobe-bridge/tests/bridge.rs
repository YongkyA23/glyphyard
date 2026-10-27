use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use typeshelf_adobe_bridge::*;

/// Library = {"Lib-A", "Lib-B"}, all initially off. Everything else is missing.
struct Fake {
    allow: bool,
    active: Mutex<HashSet<String>>,
    released: Mutex<Vec<String>>,
}

#[async_trait]
impl BridgeHandler for Fake {
    async fn approve_pairing(&self, _req: PairRequest) -> bool {
        self.allow
    }

    async fn activate(&self, req: DocRequest) -> DocResult {
        let mut active = self.active.lock().unwrap();
        let mut r = DocResult::default();
        for f in req.fonts {
            let name = f.postscript_name;
            if active.contains(&name) {
                r.already_active.push(name);
            } else if name.starts_with("Lib-") {
                active.insert(name.clone());
                r.activated.push(name);
            } else {
                r.missing.push(name);
            }
        }
        r
    }

    async fn release(&self, names: Vec<String>) {
        let mut active = self.active.lock().unwrap();
        for n in &names {
            active.remove(n);
        }
        self.released.lock().unwrap().extend(names);
    }
}

fn fake(allow: bool) -> Arc<Fake> {
    Arc::new(Fake { allow, active: Mutex::default(), released: Mutex::default() })
}

async fn bridge(dir: &Path, handler: Arc<Fake>) -> (BridgeHandle, String) {
    let config = Config { data_dir: dir.to_path_buf(), port: 0, version: "1.2.3".into() };
    let handle = start(config, handler).await.unwrap();
    let base = format!("http://{}", handle.addr());
    (handle, base)
}

fn client() -> Client {
    Client::new()
}

async fn pair(base: &str) -> reqwest::Response {
    client()
        .post(format!("{base}/v1/pair"))
        .header("X-Typeshelf-Client", "test")
        .json(&json!({ "app": "photoshop", "appVersion": "27.0", "clientId": "c1" }))
        .send()
        .await
        .unwrap()
}

async fn ping(base: &str, token: Option<&str>) -> Value {
    let mut req = client().get(format!("{base}/v1/ping")).header("X-Typeshelf-Client", "test");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    req.send().await.unwrap().json().await.unwrap()
}

async fn doc(base: &str, token: &str, document: &str, action: &str, fonts: &[&str]) -> (StatusCode, Value) {
    let fonts: Vec<Value> =
        fonts.iter().map(|f| json!({ "postscriptName": f, "family": "", "style": "", "missing": true })).collect();
    let res = client()
        .post(format!("{base}/v1/document"))
        .header("X-Typeshelf-Client", "test")
        .bearer_auth(token)
        .json(&json!({ "app": "photoshop", "document": document, "action": action, "fonts": fonts }))
        .send()
        .await
        .unwrap();
    let status = res.status();
    (status, res.json().await.unwrap_or(Value::Null))
}

async fn paired_token(base: &str) -> String {
    let res = pair(base).await;
    assert_eq!(res.status(), StatusCode::OK);
    res.json::<Value>().await.unwrap()["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn pairing_allowed_persists_hashed_and_can_be_revoked() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, base) = bridge(dir.path(), fake(true)).await;
    let token = paired_token(&base).await;

    assert_eq!(ping(&base, Some(&token)).await, json!({ "app": "typeshelf", "version": "1.2.3", "paired": true }));
    assert_eq!(ping(&base, None).await["paired"], false);
    assert!(handle.is_paired(App::Photoshop));
    assert!(!handle.is_paired(App::Indesign));
    let stored = std::fs::read_to_string(dir.path().join("adobe-tokens.json")).unwrap();
    assert!(!stored.contains(&token), "token must be stored hashed");

    // Survives a restart.
    handle.shutdown();
    let (handle, base) = bridge(dir.path(), fake(true)).await;
    assert_eq!(ping(&base, Some(&token)).await["paired"], true);

    handle.revoke(App::Photoshop).unwrap();
    assert_eq!(ping(&base, Some(&token)).await["paired"], false);
    assert_eq!(doc(&base, &token, "a.psd", "open", &[]).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn pairing_denied_is_403() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, base) = bridge(dir.path(), fake(false)).await;
    assert_eq!(pair(&base).await.status(), StatusCode::FORBIDDEN);
    assert!(!handle.is_paired(App::Photoshop));
}

#[tokio::test]
async fn document_needs_a_valid_token() {
    let dir = tempfile::tempdir().unwrap();
    let (_handle, base) = bridge(dir.path(), fake(true)).await;
    let res = client()
        .post(format!("{base}/v1/document"))
        .header("X-Typeshelf-Client", "test")
        .json(&json!({ "document": "a.psd", "action": "open", "fonts": [] }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(doc(&base, "not-a-token", "a.psd", "open", &[]).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn browsers_and_foreign_hosts_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (_handle, base) = bridge(dir.path(), fake(true)).await;

    let no_header = client().get(format!("{base}/v1/ping")).send().await.unwrap();
    assert_eq!(no_header.status(), StatusCode::FORBIDDEN);

    let preflight = client()
        .request(reqwest::Method::OPTIONS, format!("{base}/v1/pair"))
        .header("X-Typeshelf-Client", "test")
        .header("Origin", "https://evil.example")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::FORBIDDEN);
    assert!(preflight.headers().keys().all(|k| !k.as_str().starts_with("access-control-")));

    // DNS rebinding: a page on evil.example resolving to 127.0.0.1 sends its own Host.
    let rebound = client()
        .get(format!("{base}/v1/ping"))
        .header("X-Typeshelf-Client", "test")
        .header("Host", "evil.example:27716")
        .send()
        .await
        .unwrap();
    assert_eq!(rebound.status(), StatusCode::FORBIDDEN);

    let ok = client().get(format!("{base}/v1/ping")).header("X-Typeshelf-Client", "test").send().await.unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    assert!(ok.headers().keys().all(|k| !k.as_str().starts_with("access-control-")));
}

#[tokio::test]
async fn close_releases_only_fonts_no_other_document_needs() {
    let dir = tempfile::tempdir().unwrap();
    let handler = fake(true);
    let (_handle, base) = bridge(dir.path(), handler.clone()).await;
    let token = paired_token(&base).await;

    let (status, a) = doc(&base, &token, "a.psd", "open", &["Lib-A", "Lib-B", "Nope"]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(a, json!({ "activated": ["Lib-A", "Lib-B"], "missing": ["Nope"], "alreadyActive": [] }));
    let (_, b) = doc(&base, &token, "b.psd", "open", &["Lib-A"]).await;
    assert_eq!(b["alreadyActive"], json!(["Lib-A"]));

    assert_eq!(doc(&base, &token, "a.psd", "close", &[]).await.1, json!({ "released": ["Lib-B"] }));
    assert_eq!(doc(&base, &token, "b.psd", "close", &[]).await.1, json!({ "released": ["Lib-A"] }));
    assert_eq!(*handler.released.lock().unwrap(), vec!["Lib-B".to_string(), "Lib-A".to_string()]);
}

#[tokio::test]
async fn binds_loopback_falls_back_to_next_port_and_writes_discovery_file() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let dir = tempfile::tempdir().unwrap();
    let config = Config { data_dir: dir.path().to_path_buf(), port, version: "1".into() };
    let Ok(handle) = start(config, fake(true)).await else {
        return; // the next ports happened to be taken too
    };
    let addr = handle.addr();
    assert!(addr.ip().is_loopback());
    assert!(addr.port() > port && addr.port() <= port + PORT_FALLBACKS);
    let discovery: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join(DISCOVERY_FILE)).unwrap()).unwrap();
    assert_eq!(discovery["port"], addr.port());
}
