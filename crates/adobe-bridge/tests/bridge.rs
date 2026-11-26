use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use typeshelf_adobe_bridge::*;

/// Library = {"Lib-A", "Lib-B"}, all initially off. Everything else is missing.
struct Fake {
    allow: bool,
    active: Mutex<HashSet<String>>,
    released: Mutex<Vec<String>>,
    /// Apps whose process the fake reports as not running; every other app is running.
    gone: Mutex<HashSet<App>>,
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

    async fn app_running(&self, app: App) -> bool {
        !self.gone.lock().unwrap().contains(&app)
    }
}

fn fake(allow: bool) -> Arc<Fake> {
    Arc::new(Fake { allow, active: Mutex::default(), released: Mutex::default(), gone: Mutex::default() })
}

async fn bridge(dir: &Path, handler: Arc<Fake>) -> (BridgeHandle, String) {
    bridge_with_expiry(dir, handler, EXPIRY).await
}

async fn bridge_with_expiry(dir: &Path, handler: Arc<Fake>, expiry: Duration) -> (BridgeHandle, String) {
    let config = Config { data_dir: dir.to_path_buf(), port: 0, version: "1.2.3".into(), expiry };
    let handle = start(config, handler).await.unwrap();
    let base = format!("http://{}", handle.addr());
    (handle, base)
}

fn client() -> Client {
    Client::new()
}

async fn pair(base: &str) -> reqwest::Response {
    pair_as(base, "photoshop").await
}

async fn pair_as(base: &str, app: &str) -> reqwest::Response {
    pair_client(base, app, "c1").await
}

/// `client_id` tells two plug-in instances of one app apart (two Photoshop versions, say).
async fn pair_client(base: &str, app: &str, client_id: &str) -> reqwest::Response {
    client()
        .post(format!("{base}/v1/pair"))
        .header("X-Typeshelf-Client", "test")
        .json(&json!({ "app": app, "appVersion": "27.0", "clientId": client_id }))
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
    paired_token_as(base, "photoshop").await
}

async fn paired_token_as(base: &str, app: &str) -> String {
    paired_client_token(base, app, "c1").await
}

async fn paired_client_token(base: &str, app: &str, client_id: &str) -> String {
    let res = pair_client(base, app, client_id).await;
    assert_eq!(res.status(), StatusCode::OK);
    res.json::<Value>().await.unwrap()["token"].as_str().unwrap().to_string()
}

/// The plug-in's heartbeat, at 1/5 of the test expiry.
fn keep_pinging(base: &str, token: &str) -> tokio::task::JoinHandle<()> {
    let (base, token) = (base.to_string(), token.to_string());
    tokio::spawn(async move {
        loop {
            ping(&base, Some(&token)).await;
            tokio::time::sleep(SHORT_EXPIRY / 5).await;
        }
    })
}

/// Polls until `cond` holds, failing after 10 short expiries.
async fn wait_until(cond: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + SHORT_EXPIRY * 10;
    while !cond() {
        assert!(tokio::time::Instant::now() < deadline, "timed out");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

const SHORT_EXPIRY: Duration = Duration::from_secs(1);

#[tokio::test]
async fn pairing_allowed_persists_hashed_and_can_be_revoked() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, base) = bridge(dir.path(), fake(true)).await;
    let token = paired_token(&base).await;

    assert_eq!(
        ping(&base, Some(&token)).await,
        json!({ "app": "typeshelf", "version": "1.2.3", "paired": true, "documents": 0 })
    );
    assert_eq!(ping(&base, None).await, json!({ "app": "typeshelf", "version": "1.2.3", "paired": false }));
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
    let config = Config { data_dir: dir.path().to_path_buf(), port, version: "1".into(), expiry: EXPIRY };
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

#[tokio::test]
async fn silent_app_has_its_documents_closed_after_the_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let handler = fake(true);
    let (_handle, base) = bridge_with_expiry(dir.path(), handler.clone(), SHORT_EXPIRY).await;
    let photoshop = paired_token(&base).await;
    let indesign = paired_token_as(&base, "indesign").await;
    doc(&base, &photoshop, "a.psd", "open", &["Lib-A", "Lib-B"]).await;
    doc(&base, &indesign, "b.indd", "open", &["Lib-A"]).await;
    let heartbeat = keep_pinging(&base, &indesign);

    // Photoshop quits or crashes; InDesign keeps pinging and still needs Lib-A.
    handler.gone.lock().unwrap().insert(App::Photoshop);
    wait_until(|| !handler.released.lock().unwrap().is_empty()).await;
    tokio::time::sleep(SHORT_EXPIRY * 2).await;
    heartbeat.abort();
    assert_eq!(*handler.released.lock().unwrap(), vec!["Lib-B".to_string()]);
    assert_eq!(doc(&base, &indesign, "b.indd", "close", &[]).await.1, json!({ "released": ["Lib-A"] }));
}

#[tokio::test]
async fn pinging_app_keeps_its_fonts() {
    let dir = tempfile::tempdir().unwrap();
    let handler = fake(true);
    let (_handle, base) = bridge_with_expiry(dir.path(), handler.clone(), SHORT_EXPIRY).await;
    let token = paired_token(&base).await;
    doc(&base, &token, "a.psd", "open", &["Lib-A"]).await;
    let heartbeat = keep_pinging(&base, &token);
    tokio::time::sleep(SHORT_EXPIRY * 3).await;
    heartbeat.abort();
    assert!(handler.released.lock().unwrap().is_empty());
    assert_eq!(doc(&base, &token, "a.psd", "close", &[]).await.1, json!({ "released": ["Lib-A"] }));
}

/// An old plug-in never pings while connected, a closed or throttled panel goes quiet, a machine sleeps:
/// silence alone is not a quit.
#[tokio::test]
async fn silent_app_that_is_still_running_keeps_its_fonts() {
    let dir = tempfile::tempdir().unwrap();
    let handler = fake(true);
    let (_handle, base) = bridge_with_expiry(dir.path(), handler.clone(), SHORT_EXPIRY).await;
    let token = paired_token(&base).await;
    doc(&base, &token, "a.psd", "open", &["Lib-A"]).await;
    tokio::time::sleep(SHORT_EXPIRY * 4).await;
    assert!(handler.released.lock().unwrap().is_empty());
    assert_eq!(doc(&base, &token, "a.psd", "close", &[]).await.1, json!({ "released": ["Lib-A"] }));
}

#[tokio::test]
async fn fonts_owned_before_a_restart_are_released_unless_reclaimed() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, base) = bridge(dir.path(), fake(true)).await;
    let token = paired_token(&base).await;
    doc(&base, &token, "a.psd", "open", &["Lib-A"]).await;
    doc(&base, &token, "b.psd", "open", &["Lib-B"]).await;
    handle.shutdown();

    // Glyphyard restarts; the fonts are still on. Only a.psd is re-reported. The window is 3x the short
    // expiry so a slow first request on a loaded machine still lands inside it.
    let handler = fake(true);
    handler.active.lock().unwrap().extend(["Lib-A".to_string(), "Lib-B".to_string()]);
    let (_handle, base) = bridge_with_expiry(dir.path(), handler.clone(), SHORT_EXPIRY * 3).await;
    let (_, a) = doc(&base, &token, "a.psd", "open", &["Lib-A"]).await;
    assert_eq!(a["alreadyActive"], json!(["Lib-A"]));
    let heartbeat = keep_pinging(&base, &token);

    // Photoshop still runs, so b.psd may only be unreported (an old plug-in): Lib-B stays past the window.
    tokio::time::sleep(SHORT_EXPIRY * 6).await;
    assert!(handler.released.lock().unwrap().is_empty(), "kept while Photoshop runs");

    // Once Photoshop is reported gone the unclaimed font goes; Lib-A stays with the (pinging) document.
    handler.gone.lock().unwrap().insert(App::Photoshop);
    wait_until(|| !handler.released.lock().unwrap().is_empty()).await;
    tokio::time::sleep(SHORT_EXPIRY * 2).await;
    heartbeat.abort();
    assert_eq!(*handler.released.lock().unwrap(), vec!["Lib-B".to_string()]);
    assert_eq!(doc(&base, &token, "a.psd", "close", &[]).await.1, json!({ "released": ["Lib-A"] }), "still owned");
}

/// A state file from before the apps were recorded: its fonts count as held for every Adobe app, so they
/// stay on while any of them runs.
#[tokio::test]
async fn old_state_file_keeps_its_fonts_until_no_adobe_app_runs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(DISCOVERY_FILE), r#"{"port": 27716, "owned": ["Lib-A"]}"#).unwrap();
    let handler = fake(true);
    handler.active.lock().unwrap().insert("Lib-A".to_string());
    handler.gone.lock().unwrap().extend([App::Photoshop, App::Illustrator]);
    let (_handle, _base) = bridge_with_expiry(dir.path(), handler.clone(), SHORT_EXPIRY).await;

    tokio::time::sleep(SHORT_EXPIRY * 4).await;
    assert!(handler.released.lock().unwrap().is_empty(), "kept while InDesign runs");

    handler.gone.lock().unwrap().insert(App::Indesign);
    wait_until(|| !handler.released.lock().unwrap().is_empty()).await;
    assert_eq!(*handler.released.lock().unwrap(), vec!["Lib-A".to_string()]);
    let saved: Value = serde_json::from_slice(&std::fs::read(dir.path().join(DISCOVERY_FILE)).unwrap()).unwrap();
    assert_eq!(saved["owned"], json!({}));
}

/// Lets a plug-in that never saw Glyphyard go away notice it forgot its documents (restart, expiry after sleep).
#[tokio::test]
async fn authenticated_ping_reports_the_apps_open_documents() {
    let dir = tempfile::tempdir().unwrap();
    let handler = fake(true);
    let (handle, base) = bridge_with_expiry(dir.path(), handler.clone(), SHORT_EXPIRY).await;
    let photoshop = paired_token(&base).await;
    let indesign = paired_token_as(&base, "indesign").await;
    doc(&base, &photoshop, "a.psd", "open", &["Lib-A"]).await;
    doc(&base, &photoshop, "b.psd", "open", &[]).await;
    assert_eq!(ping(&base, Some(&photoshop)).await["documents"], 2);
    assert_eq!(ping(&base, Some(&indesign)).await["documents"], 0, "per app");
    assert!(ping(&base, None).await.get("documents").is_none(), "nothing for an unpaired caller");

    // Photoshop goes quiet and is reported gone (a crash) until its documents expire; its next ping (a new
    // Photoshop, say) hears the bridge has none.
    handler.gone.lock().unwrap().insert(App::Photoshop);
    wait_until(|| !handler.released.lock().unwrap().is_empty()).await;
    assert_eq!(ping(&base, Some(&photoshop)).await["documents"], 0);

    doc(&base, &photoshop, "a.psd", "open", &["Lib-A"]).await;
    assert_eq!(ping(&base, Some(&photoshop)).await["documents"], 1);
    handle.shutdown();
    let (_handle, base) = bridge(dir.path(), fake(true)).await;
    assert_eq!(ping(&base, Some(&photoshop)).await["documents"], 0, "a restarted bridge has none");
}

/// Two instances of one app (two Photoshop versions) pair as two clients. After a restart the first to
/// re-report must not hide from the second that the bridge forgot its documents.
#[tokio::test]
async fn ping_counts_only_the_calling_clients_documents() {
    let dir = tempfile::tempdir().unwrap();
    let (_handle, base) = bridge(dir.path(), fake(true)).await;
    let a = paired_client_token(&base, "photoshop", "ps-2024").await;
    let b = paired_client_token(&base, "photoshop", "ps-2025").await;
    doc(&base, &a, "a.psd", "open", &["Lib-A"]).await;
    doc(&base, &a, "b.psd", "open", &[]).await;
    assert_eq!(ping(&base, Some(&b)).await["documents"], 0, "B has re-sent nothing");
    assert_eq!(ping(&base, Some(&a)).await["documents"], 2);
}

#[tokio::test]
async fn same_named_documents_in_two_clients_hold_separate_claims() {
    let dir = tempfile::tempdir().unwrap();
    let handler = fake(true);
    let (_handle, base) = bridge(dir.path(), handler.clone()).await;
    let a = paired_client_token(&base, "photoshop", "ps-2024").await;
    let b = paired_client_token(&base, "photoshop", "ps-2025").await;
    assert_eq!(doc(&base, &a, "Untitled-1", "open", &["Lib-A"]).await.1["activated"], json!(["Lib-A"]));
    assert_eq!(doc(&base, &b, "Untitled-1", "open", &["Lib-A"]).await.1["alreadyActive"], json!(["Lib-A"]));

    assert_eq!(doc(&base, &a, "Untitled-1", "close", &[]).await.1, json!({ "released": [] }), "B still uses it");
    assert_eq!(doc(&base, &b, "Untitled-1", "close", &[]).await.1, json!({ "released": ["Lib-A"] }));
    assert_eq!(*handler.released.lock().unwrap(), vec!["Lib-A".to_string()]);
}

/// Silence is per client: one instance's heartbeat doesn't keep the other's documents alive once the app's
/// process check says it is gone.
#[tokio::test]
async fn a_silent_client_expires_while_another_client_of_its_app_pings() {
    let dir = tempfile::tempdir().unwrap();
    let handler = fake(true);
    let (_handle, base) = bridge_with_expiry(dir.path(), handler.clone(), SHORT_EXPIRY).await;
    let a = paired_client_token(&base, "photoshop", "ps-2024").await;
    let b = paired_client_token(&base, "photoshop", "ps-2025").await;
    doc(&base, &a, "a.psd", "open", &["Lib-A"]).await;
    doc(&base, &b, "b.psd", "open", &["Lib-B"]).await;
    let heartbeat = keep_pinging(&base, &b);

    handler.gone.lock().unwrap().insert(App::Photoshop);
    wait_until(|| !handler.released.lock().unwrap().is_empty()).await;
    tokio::time::sleep(SHORT_EXPIRY * 2).await;
    heartbeat.abort();
    assert_eq!(*handler.released.lock().unwrap(), vec!["Lib-A".to_string()], "B is heard from, so b.psd stays");
    assert_eq!(ping(&base, Some(&b)).await["documents"], 1);
}
