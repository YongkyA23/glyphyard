//! Drives the real router against an in-memory database.

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use chrono::{DateTime, Duration, Utc};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tower::ServiceExt;
use typeshelf_protocol::{ClientMessage, CommandKind, ServerMessage};
use typeshelf_server::{admin, app, auth, db, AppState};

struct Server {
    app: Router,
    state: AppState,
    key: String,
    cookie: String,
    _dir: tempfile::TempDir,
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

impl Server {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new(db::open(None).unwrap(), dir.path().to_path_buf());
        let key = db::settings(&state.db()).unwrap().install_key;
        admin::insert_admin(&state.db(), "admin@example.com", &auth::hash_password("hunter2hunter2"), "owner").unwrap();
        let app = app(state.clone(), &dir.path().join("static"));
        let mut s = Server { app, state, key, cookie: String::new(), _dir: dir };
        let r = s.send(Method::POST, "/api/admin/login", &[], Some(json!({ "email": "admin@example.com", "password": "hunter2hunter2" }))).await;
        assert_eq!(r.status, StatusCode::NO_CONTENT);
        let set_cookie = r.headers[header::SET_COOKIE].to_str().unwrap();
        s.cookie = set_cookie.split(';').next().unwrap().to_string();
        s
    }

    async fn send(&self, method: Method, path: &str, headers: &[(&str, &str)], body: Option<Value>) -> Reply {
        let mut req = Request::builder().method(method).uri(path);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let req = match body {
            Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        self.raw(req).await
    }

    async fn raw(&self, req: Request<Body>) -> Reply {
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap().to_vec();
        Reply { status, headers, body }
    }

    async fn desktop(&self, method: Method, path: &str, token: &str, body: Option<Value>) -> Reply {
        let bearer = format!("Bearer {token}");
        self.send(method, &format!("/api/v1{path}"), &[("X-Install-Key", &self.key), ("Authorization", &bearer)], body).await
    }

    async fn admin(&self, method: Method, path: &str, body: Option<Value>) -> Reply {
        self.send(method, &format!("/api/admin{path}"), &[("Cookie", &self.cookie)], body).await
    }

    async fn register(&self, id: &str, name: &str) -> String {
        let body = json!({ "install_id": id, "name": name, "os": "windows", "app_version": "0.1.0" });
        let r = self.send(Method::POST, "/api/v1/installs", &[("X-Install-Key", &self.key)], Some(body)).await;
        assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.body));
        r.json()["token"].as_str().unwrap().to_string()
    }

    async fn commands(&self, token: &str) -> Vec<Value> {
        self.desktop(Method::GET, "/me/commands", token, None).await.json().as_array().unwrap().clone()
    }

    /// A library font: a real system font via upload when one exists, otherwise a row seeded directly.
    async fn library_font(&self) -> String {
        let candidates = ["C:\\Windows\\Fonts\\arial.ttf", "/System/Library/Fonts/Supplemental/Arial.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"];
        if let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) {
            let boundary = "XBOUNDARYX";
            let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"Arial.ttf\"\r\nContent-Type: font/ttf\r\n\r\n").into_bytes();
            body.extend_from_slice(&bytes);
            body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
            let req = Request::post("/api/admin/library")
                .header("Cookie", &self.cookie)
                .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                .body(Body::from(body))
                .unwrap();
            let r = self.raw(req).await;
            assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.body));
            let files = r.json();
            assert_eq!(files[0]["delivered_to"], "none");
            return files[0]["hash"].as_str().unwrap().to_string();
        }
        let bytes = b"not really a font";
        let hash = typeshelf_core::file_hash(bytes);
        std::fs::create_dir_all(self.state.data_dir.join("library")).unwrap();
        std::fs::write(self.state.data_dir.join("library").join(&hash), bytes).unwrap();
        self.state
            .db()
            .execute(
                "INSERT INTO library_files (hash, filename, size, ps_name, family, style, version, uploaded_at)
                 VALUES (?1, 'Fake.ttf', 17, 'Fake-Regular', 'Fake', 'Regular', '1.0', ?2)",
                [hash.clone(), db::now()],
            )
            .unwrap();
        hash
    }
}

fn font(hash: &str) -> Value {
    json!({ "hash": hash, "ps_name": "Fraunces-Bold", "family": "Fraunces", "style": "Bold" })
}

fn event(kind: Value) -> Value {
    let mut e = kind;
    e["at"] = json!(Utc::now());
    json!({ "events": [e] })
}

#[tokio::test]
async fn install_key_is_checked() {
    let s = Server::new().await;
    let bad = s.send(Method::GET, "/api/v1/health", &[("X-Install-Key", "nope")], None).await;
    assert_eq!(bad.status, StatusCode::FORBIDDEN);
    assert_eq!(bad.json()["error"], "bad_install_key");
    let missing = s.send(Method::POST, "/api/v1/installs", &[], Some(json!({}))).await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN);
    let ok = s.send(Method::GET, "/api/v1/health", &[("X-Install-Key", &s.key)], None).await;
    assert_eq!(ok.status, StatusCode::NO_CONTENT);
    let no_token = s.send(Method::GET, "/api/v1/me", &[("X-Install-Key", &s.key)], None).await;
    assert_eq!(no_token.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn register_then_duplicate_is_409() {
    let s = Server::new().await;
    let token = s.register("inst-1", "  Budi   Santoso ").await;
    let me = s.desktop(Method::GET, "/me", &token, None).await.json();
    assert_eq!(me["name"], "Budi Santoso");
    assert_eq!(me["next_name_change"], Value::Null, "the first name does not count as a change");
    assert_eq!(me["policy"]["name_change_days"], 30);

    let body = json!({ "install_id": "inst-1", "name": "Other", "os": "windows", "app_version": "0.1.0" });
    let again = s.send(Method::POST, "/api/v1/installs", &[("X-Install-Key", &s.key)], Some(body)).await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.json()["error"], "already_registered");
}

#[tokio::test]
async fn name_is_locked_until_admin_resets() {
    let s = Server::new().await;
    let token = s.register("inst-1", "Budi").await;
    let first = s.desktop(Method::PUT, "/me/name", &token, Some(json!({ "name": "Budi Santoso" }))).await;
    assert_eq!(first.status, StatusCode::OK);
    let next: DateTime<Utc> = serde_json::from_value(first.json()["next_name_change"].clone()).unwrap();
    assert!((next - (Utc::now() + Duration::days(30))).num_seconds().abs() < 60);

    let locked = s.desktop(Method::PUT, "/me/name", &token, Some(json!({ "name": "Budi S." }))).await;
    assert_eq!(locked.status, StatusCode::CONFLICT);
    assert_eq!(locked.json()["error"], "name_locked");
    let locked_next: DateTime<Utc> = serde_json::from_value(locked.json()["next_name_change"].clone()).unwrap();
    assert_eq!(locked_next, next);

    assert_eq!(s.admin(Method::POST, "/installs/inst-1/reset-name-limit", None).await.status, StatusCode::NO_CONTENT);
    let after = s.desktop(Method::PUT, "/me/name", &token, Some(json!({ "name": "Budi S." }))).await;
    assert_eq!(after.status, StatusCode::OK);
    assert_eq!(after.json()["name"], "Budi S.");

    let names = s.admin(Method::GET, "/installs/inst-1/timeline?kind=names", None).await.json();
    assert_eq!(names.as_array().unwrap().len(), 4); // joined, change, reset, change
}

#[tokio::test]
async fn admin_auth() {
    let s = Server::new().await;
    let bad = s.send(Method::POST, "/api/admin/login", &[], Some(json!({ "email": "admin@example.com", "password": "wrong" }))).await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    assert_eq!(bad.json()["error"], "bad_credentials");
    assert_eq!(s.send(Method::GET, "/api/admin/me", &[], None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(s.send(Method::GET, "/api/admin/installs", &[], None).await.status, StatusCode::UNAUTHORIZED);
    let me = s.admin(Method::GET, "/me", None).await;
    assert_eq!(me.json(), json!({ "email": "admin@example.com", "role": "owner" }));

    assert_eq!(s.admin(Method::POST, "/admins", Some(json!({ "email": "two@example.com", "password": "longenough" }))).await.status, StatusCode::CREATED);
    assert_eq!(s.admin(Method::GET, "/admins", None).await.json().as_array().unwrap().len(), 2);

    assert_eq!(s.admin(Method::POST, "/logout", None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(s.admin(Method::GET, "/me", None).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn remote_deactivate_arrives_over_websocket() {
    let s = Server::new().await;
    let token = s.register("inst-1", "Budi").await;
    let activated = event(json!({ "kind": "font_activated", "font": font("h1"), "source": "user" }));
    assert_eq!(s.desktop(Method::POST, "/me/events", &token, Some(activated)).await.status, StatusCode::NO_CONTENT);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = s.app.clone();
    tokio::spawn(async move { axum::serve(listener, router).await });
    let url = format!("ws://{addr}/api/v1/me/ws?key={}&token={token}", s.key);
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();

    // The socket is registered by the time the server has answered the upgrade, but give the task a moment.
    for _ in 0..50 {
        if s.state.hub.is_connected("inst-1") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let installs = s.admin(Method::GET, "/installs?filter=online", None).await.json();
    assert_eq!(installs[0]["online"], true);

    let r = s.admin(Method::POST, "/installs/inst-1/fonts", Some(json!({ "hash": "h1", "active": false }))).await;
    assert_eq!(r.status, StatusCode::OK);
    let command_id = r.json()["command_id"].as_i64().unwrap();

    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await.unwrap().unwrap().unwrap();
    let ServerMessage::Command(cmd) = serde_json::from_str(msg.to_text().unwrap()).unwrap();
    assert_eq!(cmd.id, command_id);
    assert!(matches!(cmd.kind, CommandKind::Deactivate { ref font } if font.hash == "h1" && font.family == "Fraunces"));

    let ack = serde_json::to_string(&ClientMessage::Ack { id: command_id, ok: true, error: None }).unwrap();
    ws.send(tokio_tungstenite::tungstenite::Message::Text(ack.into())).await.unwrap();
    for _ in 0..50 {
        if s.commands(&token).await.is_empty() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the ack was not recorded");
}

#[tokio::test]
async fn user_turning_off_admin_font_is_an_override() {
    let s = Server::new().await;
    let token = s.register("inst-1", "Budi").await;
    let on = event(json!({ "kind": "font_activated", "font": font("h1"), "source": "admin" }));
    s.desktop(Method::POST, "/me/events", &token, Some(on)).await;
    let off = event(json!({ "kind": "font_deactivated", "font": font("h1"), "source": "user", "was_admin": false }));
    s.desktop(Method::POST, "/me/events", &token, Some(off)).await;

    let overrides = s.admin(Method::GET, "/installs/inst-1/timeline?kind=overrides", None).await.json();
    assert_eq!(overrides.as_array().unwrap().len(), 1);
    assert_eq!(overrides[0]["by"], "user");

    let detail = s.admin(Method::GET, "/installs/inst-1", None).await.json();
    assert_eq!(detail["overrides"].as_array().unwrap().len(), 1);
    assert_eq!(detail["fonts"][0]["overridden"], true);
    assert!(detail["flags"].as_array().unwrap().contains(&json!("override")));

    // A snapshot keeps the override visible; turning it back on clears it.
    s.desktop(Method::PUT, "/me/fonts", &token, Some(json!({ "active": [] }))).await;
    assert_eq!(s.admin(Method::GET, "/installs/inst-1", None).await.json()["overrides"].as_array().unwrap().len(), 1);
    let snap = json!({ "active": [{ "hash": "h1", "ps_name": "Fraunces-Bold", "family": "Fraunces", "style": "Bold", "source": "admin" }] });
    s.desktop(Method::PUT, "/me/fonts", &token, Some(snap)).await;
    let detail = s.admin(Method::GET, "/installs/inst-1", None).await.json();
    assert_eq!(detail["overrides"].as_array().unwrap().len(), 0);
    assert_eq!(detail["active_fonts"], 1);
}

#[tokio::test]
async fn merge_carries_names_and_limit_then_split() {
    let s = Server::new().await;
    let old = s.register("old", "Budi").await;
    let new = s.register("new", "Budi S.").await;
    assert_eq!(s.desktop(Method::PUT, "/me/name", &new, Some(json!({ "name": "Budi Santoso" }))).await.status, StatusCode::OK);

    let list = s.admin(Method::GET, "/installs?filter=duplicates", None).await.json();
    assert_eq!(list.as_array().unwrap().len(), 2);

    // The request names the newer install as primary; the older one still wins.
    let merged = s.admin(Method::POST, "/installs/merge", Some(json!({ "primary": "new", "secondary": "old", "keep_name": "primary" }))).await;
    assert_eq!(merged.status, StatusCode::OK, "{}", String::from_utf8_lossy(&merged.body));
    let d = merged.json();
    assert_eq!(d["id"], "old");
    assert_eq!(d["name"], "Budi Santoso");
    assert_eq!(d["merged_from"], json!(["new"]));
    let history: Vec<&str> = d["name_history"].as_array().unwrap().iter().map(|h| h["name"].as_str().unwrap()).collect();
    assert_eq!(history, ["Budi", "Budi S.", "Budi Santoso", "Budi Santoso"]);
    assert!(d["next_name_change"].is_string(), "the secondary's recent change locks the merged install");
    let locked = s.desktop(Method::PUT, "/me/name", &old, Some(json!({ "name": "B" }))).await;
    assert_eq!(locked.status, StatusCode::CONFLICT);

    let list = s.admin(Method::GET, "/installs?q=budi%20s.", None).await.json();
    assert_eq!(list.as_array().unwrap().len(), 1, "merged installs leave the list; old names still match");

    assert_eq!(s.admin(Method::POST, "/installs/new/split", None).await.status, StatusCode::NO_CONTENT);
    let d = s.admin(Method::GET, "/installs/old", None).await.json();
    assert_eq!(d["name"], "Budi");
    assert_eq!(d["merged_from"], json!([]));
    assert_eq!(s.admin(Method::GET, "/installs", None).await.json().as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn block_is_pushed_to_every_install() {
    let s = Server::new().await;
    let a = s.register("a", "Ana").await;
    let b = s.register("b", "Budi").await;
    let hash = s.library_font().await;
    let r = s.admin(Method::POST, &format!("/library/{hash}/block"), Some(json!({ "blocked": true }))).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["blocked"], true);
    for token in [&a, &b] {
        let cmds = s.commands(token).await;
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0]["kind"], "block");
        assert_eq!(cmds[0]["hash"], hash.as_str());
    }
    // New installs learn about blocks too, and remote activation is refused.
    let c = s.register("c", "Citra").await;
    assert_eq!(s.commands(&c).await[0]["kind"], "block");
    let r = s.admin(Method::POST, "/installs/a/fonts", Some(json!({ "hash": hash, "active": true }))).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn library_download_needs_a_delivery() {
    let s = Server::new().await;
    let a = s.register("a", "Ana").await;
    let b = s.register("b", "Budi").await;
    let hash = s.library_font().await;
    let path = format!("/library/files/{hash}");
    assert_eq!(s.desktop(Method::GET, &path, &a, None).await.status, StatusCode::NOT_FOUND);

    let r = s.admin(Method::POST, &format!("/library/{hash}/deliver"), Some(json!({ "target": ["a"], "activate": true }))).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["delivered_to"], json!(["a"]));
    let cmds = s.commands(&a).await;
    assert_eq!(cmds[0]["kind"], "deliver");
    assert_eq!(cmds[0]["activate"], true);

    let got = s.desktop(Method::GET, &path, &a, None).await;
    assert_eq!(got.status, StatusCode::OK);
    assert_eq!(got.body, std::fs::read(s.state.data_dir.join("library").join(&hash)).unwrap());
    assert_eq!(s.desktop(Method::GET, "/me/library", &a, None).await.json().as_array().unwrap().len(), 1);
    assert_eq!(s.desktop(Method::GET, &path, &b, None).await.status, StatusCode::NOT_FOUND);
    assert!(s.desktop(Method::GET, "/me/library", &b, None).await.json().as_array().unwrap().is_empty());

    s.admin(Method::POST, &format!("/library/{hash}/block"), Some(json!({ "blocked": true }))).await;
    assert_eq!(s.desktop(Method::GET, &path, &a, None).await.status, StatusCode::NOT_FOUND);

    // A single file has nothing to organize.
    assert_eq!(s.admin(Method::GET, "/library/organize", None).await.json(), json!({ "changes": [] }));
    assert_eq!(s.admin(Method::POST, "/library/organize/apply", None).await.json(), json!({ "applied": 0 }));
}

#[tokio::test]
async fn delivering_to_all_reaches_new_installs() {
    let s = Server::new().await;
    let hash = s.library_font().await;
    s.admin(Method::POST, &format!("/library/{hash}/deliver"), Some(json!({ "target": "all" }))).await;
    let late = s.register("late", "Dewi").await;
    let cmds = s.commands(&late).await;
    assert_eq!(cmds[0]["kind"], "deliver");
    assert_eq!(cmds[0]["activate"], false, "defaults to the activate_delivered setting");
    assert_eq!(s.desktop(Method::GET, &format!("/library/files/{hash}"), &late, None).await.status, StatusCode::OK);
}

#[tokio::test]
async fn settings_change_pushes_policy() {
    let s = Server::new().await;
    let token = s.register("a", "Ana").await;
    let r = s.admin(Method::PUT, "/settings", Some(json!({ "name_change_days": 7, "install_key": "ignored" }))).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["install_key"], s.key.as_str());
    let cmds = s.commands(&token).await;
    assert_eq!(cmds[0]["kind"], "policy_changed");
    assert_eq!(cmds[0]["policy"]["name_change_days"], 7);

    let new_key = s.admin(Method::POST, "/settings/install-key", None).await.json()["install_key"].as_str().unwrap().to_string();
    assert_ne!(new_key, s.key);
    assert_eq!(s.desktop(Method::GET, "/me", &token, None).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn name_changes_csv() {
    let s = Server::new().await;
    let token = s.register("inst-1", "Budi").await;
    s.desktop(Method::PUT, "/me/name", &token, Some(json!({ "name": "Santoso, Budi" }))).await;

    let rows = s.admin(Method::GET, "/name-changes", None).await.json();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert_eq!(rows[0]["by"], "user");
    assert!(rows[0]["next_allowed"].is_string());
    assert_eq!(rows[1]["by"], "first");

    let csv = s.admin(Method::GET, "/name-changes.csv?q=santoso", None).await;
    assert_eq!(csv.status, StatusCode::OK);
    assert!(csv.headers[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/csv"));
    let text = String::from_utf8(csv.body).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "at,install_ids,old_name,new_name,by,next_allowed");
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains(",inst-1,Budi,\"Santoso, Budi\",user,"), "{}", lines[1]);
}

#[tokio::test]
async fn overview_and_spa_fallback() {
    let s = Server::new().await;
    let token = s.register("a", "Ana").await;
    let report = event(json!({ "kind": "duplicates_report", "duplicates": 3, "version_conflicts": 1, "name_clashes": 0 }));
    s.desktop(Method::POST, "/me/events", &token, Some(report)).await;
    let o = s.admin(Method::GET, "/overview", None).await.json();
    assert_eq!(o["installs"], 1);
    assert_eq!(o["os"]["windows"], 1);
    assert_eq!(o["recent"].as_array().unwrap().len(), 2);
    let reports = s.admin(Method::GET, "/organize/reports", None).await.json();
    assert_eq!(reports[0]["duplicates"], 3);

    let page = s.send(Method::GET, "/installs/a", &[], None).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&page.body).contains("not built"));
}
