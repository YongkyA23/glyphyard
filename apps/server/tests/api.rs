//! Drives the real router against an in-memory database.

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use chrono::{DateTime, Duration, Utc};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tower::ServiceExt;
use std::sync::LazyLock;
use typeshelf_protocol::{ClientMessage, CommandKind, ServerMessage, FONT_NOT_HERE_CODE, FONT_NOT_HERE_SUFFIX};
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

    /// Acks every pending command and returns their kinds.
    async fn ack_all(&self, token: &str) -> Vec<String> {
        let mut kinds = Vec::new();
        for c in self.commands(token).await {
            let id = c["id"].as_i64().unwrap();
            self.desktop(Method::POST, &format!("/me/commands/{id}/ack"), token, Some(json!({ "ok": true }))).await;
            kinds.push(c["kind"].as_str().unwrap().to_string());
        }
        kinds
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
async fn registering_again_before_first_use_reissues_the_token() {
    let s = Server::new().await;
    let hash = s.library_font().await;
    s.admin(Method::POST, &format!("/library/{hash}/block"), Some(json!({ "blocked": true }))).await;
    let lost = s.register("inst-1", "Budi").await; // the desktop never received this response
    let token = s.register("inst-1", "Budi").await;
    assert_ne!(token, lost);
    assert_eq!(s.desktop(Method::GET, "/me", &lost, None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(s.desktop(Method::GET, "/me", &token, None).await.json()["name"], "Budi");
    assert_eq!(s.commands(&token).await.len(), 1, "welcome commands are not queued twice");

    // Once the install has used its token, the id is taken for good.
    let body = json!({ "install_id": "inst-1", "name": "Budi", "os": "windows", "app_version": "0.1.0" });
    let again = s.send(Method::POST, "/api/v1/installs", &[("X-Install-Key", &s.key)], Some(body)).await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(s.desktop(Method::GET, "/me", &token, None).await.status, StatusCode::OK);
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
    assert_eq!(names["items"].as_array().unwrap().len(), 4); // joined, change, reset, change
    assert_eq!(names["total"], 4);
    let page = s.admin(Method::GET, "/installs/inst-1/timeline?kind=names&offset=3&limit=2", None).await.json();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["total"], 4);
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
async fn failed_logins_are_throttled_per_email() {
    let s = Server::new().await;
    let login = |email: &'static str, password: &'static str| {
        s.send(Method::POST, "/api/admin/login", &[], Some(json!({ "email": email, "password": password })))
    };
    // A success clears the count: 4 failures, a success, 4 more failures are all still 401.
    for _ in 0..4 {
        assert_eq!(login("admin@example.com", "wrong").await.status, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(login("admin@example.com", "hunter2hunter2").await.status, StatusCode::NO_CONTENT);
    for _ in 0..4 {
        assert_eq!(login("admin@example.com", "wrong").await.status, StatusCode::UNAUTHORIZED);
    }
    // The 5th failure in the window locks the email (compared trimmed and lowercased), even with the right password.
    assert_eq!(login(" Admin@Example.com ", "wrong").await.status, StatusCode::UNAUTHORIZED);
    let locked = login("admin@example.com", "hunter2hunter2").await;
    assert_eq!(locked.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(locked.json()["error"], "too_many_attempts");
    // Unknown emails count the same way, so the lock does not reveal which emails exist.
    for _ in 0..5 {
        assert_eq!(login("nobody@example.com", "wrong").await.status, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(login("nobody@example.com", "wrong").await.status, StatusCode::TOO_MANY_REQUESTS);
    // Other emails are unaffected.
    assert_eq!(login("someone@example.com", "wrong").await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn session_cookie_is_secure_over_https() {
    let s = Server::new().await;
    let creds = || Some(json!({ "email": "admin@example.com", "password": "hunter2hunter2" }));
    let cookie = |r: &Reply| r.headers[header::SET_COOKIE].to_str().unwrap().to_string();

    let plain = s.send(Method::POST, "/api/admin/login", &[], creds()).await;
    assert!(!cookie(&plain).contains("Secure"), "{}", cookie(&plain));
    let proxied = s.send(Method::POST, "/api/admin/login", &[("X-Forwarded-Proto", "https")], creds()).await;
    assert!(cookie(&proxied).ends_with("; Secure"), "{}", cookie(&proxied));
    let logout = s.send(Method::POST, "/api/admin/logout", &[("X-Forwarded-Proto", "https")], None).await;
    assert!(cookie(&logout).ends_with("; Secure"), "{}", cookie(&logout));

    // A configured https public URL marks the cookie Secure without the proxy header, for requests to that host.
    assert_eq!(s.admin(Method::PUT, "/settings", Some(json!({ "server_url": "https://fonts.example.com" }))).await.status, StatusCode::OK);
    let public = [("Host", "fonts.example.com")];
    let configured = s.send(Method::POST, "/api/admin/login", &public, creds()).await;
    assert!(cookie(&configured).ends_with("; Secure"), "{}", cookie(&configured));
    assert!(cookie(&s.send(Method::POST, "/api/admin/logout", &public, None).await).ends_with("; Secure"));
    let spelled = s.send(Method::POST, "/api/admin/login", &[("Host", "Fonts.Example.com:443")], creds()).await;
    assert!(cookie(&spelled).ends_with("; Secure"), "{}", cookie(&spelled));

    // The same server opened by its LAN address over plain HTTP: a Secure cookie would be dropped.
    let lan = s.send(Method::POST, "/api/admin/login", &[("Host", "192.168.1.10:8095")], creds()).await;
    assert!(!cookie(&lan).contains("Secure"), "{}", cookie(&lan));
    let lan_proxied = s.send(Method::POST, "/api/admin/login", &[("Host", "192.168.1.10:8095"), ("X-Forwarded-Proto", "https")], creds()).await;
    assert!(cookie(&lan_proxied).ends_with("; Secure"), "{}", cookie(&lan_proxied));
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
    assert_eq!(installs["items"][0]["online"], true);

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
    assert_eq!(overrides["items"].as_array().unwrap().len(), 1);
    assert_eq!(overrides["items"][0]["by"], "user");

    let detail = s.admin(Method::GET, "/installs/inst-1", None).await.json();
    assert_eq!(detail["overrides"].as_array().unwrap().len(), 1);
    assert!(detail.get("fonts").is_none(), "fonts are paged separately");
    let fonts = s.admin(Method::GET, "/installs/inst-1/fonts", None).await.json();
    assert_eq!(fonts["total"], 1);
    assert_eq!(fonts["items"][0]["overridden"], true);
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
    assert_eq!(list["total"], 2);

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
    assert_eq!(list["items"].as_array().unwrap().len(), 1, "merged installs leave the list; old names still match");

    assert_eq!(s.admin(Method::POST, "/installs/new/split", None).await.status, StatusCode::NO_CONTENT);
    let d = s.admin(Method::GET, "/installs/old", None).await.json();
    assert_eq!(d["name"], "Budi");
    assert_eq!(d["merged_from"], json!([]));
    assert_eq!(s.admin(Method::GET, "/installs", None).await.json()["total"], 2);
}

#[tokio::test]
async fn merged_installs_still_get_commands_and_share_a_font_list() {
    let s = Server::new().await;
    let old = s.register("old", "Budi").await;
    let new = s.register("new", "Budi S.").await;
    let other = s.register("other", "Citra").await;
    for (token, e) in [
        (&old, json!({ "kind": "font_activated", "font": font("h1"), "source": "user" })),
        (&new, json!({ "kind": "font_activated", "font": font("h1"), "source": "user" })),
        (&new, json!({ "kind": "font_deactivated", "font": font("h1"), "source": "user", "was_admin": false })),
        (&new, json!({ "kind": "font_activated", "font": font("h2"), "source": "user" })),
    ] {
        assert_eq!(s.desktop(Method::POST, "/me/events", token, Some(event(e))).await.status, StatusCode::NO_CONTENT);
    }
    let merged = s.admin(Method::POST, "/installs/merge", Some(json!({ "primary": "old", "secondary": "new", "keep_name": "primary" }))).await;
    assert_eq!(merged.status, StatusCode::OK);

    // Commands aimed at the surviving install reach every computer in its group, and only those.
    let hash = s.library_font().await;
    assert_eq!(s.admin(Method::POST, "/installs/old/fonts", Some(json!({ "hash": "h2", "active": false }))).await.status, StatusCode::OK);
    assert_eq!(s.admin(Method::POST, "/installs/old/organize-request", None).await.status, StatusCode::OK);
    s.admin(Method::POST, &format!("/library/{hash}/deliver"), Some(json!({ "target": ["old"] }))).await;
    for token in [&old, &new] {
        assert_eq!(s.ack_all(token).await, ["deactivate", "organize_request", "deliver"]);
        assert_eq!(s.desktop(Method::GET, &format!("/library/files/{hash}"), token, None).await.status, StatusCode::OK);
    }
    assert!(s.commands(&other).await.is_empty());

    // Broadcasts reach the merged install too.
    s.admin(Method::POST, &format!("/library/{hash}/block"), Some(json!({ "blocked": true }))).await;
    s.admin(Method::PUT, "/settings", Some(json!({ "name_change_days": 7 }))).await;
    s.admin(Method::POST, &format!("/library/{hash}/block"), Some(json!({ "blocked": false }))).await;
    s.admin(Method::POST, &format!("/library/{hash}/deliver"), Some(json!({ "target": "all" }))).await;
    for token in [&old, &new, &other] {
        assert_eq!(s.ack_all(token).await, ["block", "policy_changed", "unblock", "deliver"]);
    }

    // The font list covers the group: one row per font, on if any computer has it on.
    let fonts = s.admin(Method::GET, "/installs/old/fonts", None).await.json();
    assert_eq!(fonts["total"], 2);
    let rows: Vec<(&str, bool)> = fonts["items"].as_array().unwrap().iter().map(|f| (f["hash"].as_str().unwrap(), f["active"].as_bool().unwrap())).collect();
    assert_eq!(rows, [("h1", true), ("h2", true)]);
    assert_eq!(s.admin(Method::GET, "/installs/old/fonts?active=false", None).await.json()["total"], 0);
}

/// What a desktop acks when the font is not in its library (`EngineError` display: `code: message`), built from
/// the wording the engine uses, so the server's matcher cannot drift from it.
static NOT_HERE: LazyLock<String> = LazyLock::new(|| format!("{FONT_NOT_HERE_CODE}: Fraunces-Bold{FONT_NOT_HERE_SUFFIX}"));

impl Server {
    /// The one pending command's id; acks it with `error` (`None` = success).
    async fn ack_one(&self, token: &str, error: Option<&str>) {
        let cmds = self.commands(token).await;
        assert_eq!(cmds.len(), 1);
        let id = cmds[0]["id"].as_i64().unwrap();
        let r = self.desktop(Method::POST, &format!("/me/commands/{id}/ack"), token, Some(json!({ "ok": error.is_none(), "error": error }))).await;
        assert_eq!(r.status, StatusCode::NO_CONTENT);
    }

    async fn failures(&self, id: &str) -> (i64, bool) {
        let total = self.admin(Method::GET, &format!("/installs/{id}/timeline?kind=failures"), None).await.json()["total"].as_i64().unwrap();
        let flags = self.admin(Method::GET, &format!("/installs/{id}"), None).await.json()["flags"].clone();
        (total, flags.as_array().unwrap().contains(&json!("failures")))
    }
}

#[tokio::test]
async fn merged_group_fails_a_missing_font_only_when_no_computer_has_it() {
    let s = Server::new().await;
    let old = s.register("old", "Budi").await;
    let new = s.register("new", "Budi S.").await;
    s.desktop(Method::POST, "/me/events", &old, Some(event(json!({ "kind": "font_activated", "font": font("h1"), "source": "user" })))).await;
    s.admin(Method::POST, "/installs/merge", Some(json!({ "primary": "old", "secondary": "new", "keep_name": "primary" }))).await;
    // Both computers have the command (fetching it delivers it).
    let activate = || async {
        assert_eq!(s.admin(Method::POST, "/installs/old/fonts", Some(json!({ "hash": "h1", "active": true }))).await.status, StatusCode::OK);
        s.commands(&old).await;
        s.commands(&new).await;
    };

    // Fail then succeed, and succeed then fail: the font is on where it exists, so no failure.
    activate().await;
    s.ack_one(&new, Some(&NOT_HERE)).await;
    s.ack_one(&old, None).await;
    activate().await;
    s.ack_one(&old, None).await;
    s.ack_one(&new, Some(&NOT_HERE)).await;
    assert_eq!(s.failures("old").await, (0, false));

    // Nobody has it: one failure for the group, once the last computer answers.
    activate().await;
    s.ack_one(&new, Some(&NOT_HERE)).await;
    assert_eq!(s.failures("old").await, (0, false), "the other computer may still have it");
    s.ack_one(&old, Some(&NOT_HERE)).await;
    assert_eq!(s.failures("old").await, (1, true));
}

#[tokio::test]
async fn merged_group_fails_at_once_when_the_other_computer_never_got_the_command() {
    let s = Server::new().await;
    let old = s.register("old", "Budi").await;
    s.register("new", "Budi S.").await;
    s.desktop(Method::POST, "/me/events", &old, Some(event(json!({ "kind": "font_activated", "font": font("h1"), "source": "user" })))).await;
    s.admin(Method::POST, "/installs/merge", Some(json!({ "primary": "old", "secondary": "new", "keep_name": "primary" }))).await;
    s.admin(Method::POST, "/installs/old/fonts", Some(json!({ "hash": "h1", "active": true }))).await;
    // "new" is a dead install: it never connects, so its copy is never delivered.
    s.ack_one(&old, Some(&NOT_HERE)).await;
    assert_eq!(s.failures("old").await, (1, true));
    let text = &s.admin(Method::GET, "/installs/old/timeline?kind=failures", None).await.json()["items"][0]["text"];
    assert_eq!(text, &json!(format!("Admin command “activate” failed: {}", *NOT_HERE)));
}

#[tokio::test]
async fn a_single_install_missing_a_font_is_a_failure_as_before() {
    let s = Server::new().await;
    let token = s.register("solo", "Citra").await;
    s.desktop(Method::POST, "/me/events", &token, Some(event(json!({ "kind": "font_activated", "font": font("h1"), "source": "user" })))).await;
    s.admin(Method::POST, "/installs/solo/fonts", Some(json!({ "hash": "h1", "active": true }))).await;
    s.ack_one(&token, Some(&NOT_HERE)).await;
    assert_eq!(s.failures("solo").await, (1, true));
    // A repeated ack (socket ack retried over REST) is accepted but recorded once.
    let id: i64 = s.state.db().query_row("SELECT MAX(id) FROM commands", [], |r| r.get(0)).unwrap();
    let again = s.desktop(Method::POST, &format!("/me/commands/{id}/ack"), &token, Some(json!({ "ok": false, "error": *NOT_HERE }))).await;
    assert_eq!(again.status, StatusCode::NO_CONTENT);
    assert_eq!(s.failures("solo").await, (1, true));
    let text = &s.admin(Method::GET, "/installs/solo/timeline?kind=failures", None).await.json()["items"][0]["text"];
    assert_eq!(text, &json!(format!("Admin command “activate” failed: {}", *NOT_HERE)));
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

/// Desktops unblock by PostScript name, so unblocking one of two blocked files must re-block the other.
#[tokio::test]
async fn unblocking_one_file_keeps_its_namesake_blocked() {
    let s = Server::new().await;
    let a = s.register("a", "Ana").await;
    for hash in ["h1", "h2"] {
        s.state
            .db()
            .execute(
                "INSERT INTO library_files (hash, filename, size, ps_name, family, style, version, uploaded_at)
                 VALUES (?1, ?1 || '.ttf', 17, 'Fake-Regular', 'Fake', 'Regular', '1.0', ?2)",
                [hash.to_string(), db::now()],
            )
            .unwrap();
        assert_eq!(s.admin(Method::POST, &format!("/library/{hash}/block"), Some(json!({ "blocked": true }))).await.status, StatusCode::OK);
    }
    s.ack_all(&a).await;

    assert_eq!(s.admin(Method::POST, "/library/h1/block", Some(json!({ "blocked": false }))).await.status, StatusCode::OK);
    let cmds = s.commands(&a).await;
    let kinds: Vec<(&str, &str)> = cmds.iter().map(|c| (c["kind"].as_str().unwrap(), c["hash"].as_str().unwrap_or(""))).collect();
    assert_eq!(kinds, [("unblock", ""), ("block", "h2")]);
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

    let lib = s.admin(Method::GET, "/library", None).await.json();
    assert_eq!(lib["total"], 1);
    assert_eq!(lib["items"][0]["hash"], hash.as_str());
    assert_eq!(s.admin(Method::GET, "/library?q=nothing-like-this", None).await.json()["total"], 0);

    // A single file has nothing to organize.
    assert_eq!(s.admin(Method::GET, "/library/organize", None).await.json(), json!({ "changes": [], "notes": [] }));
    assert_eq!(s.admin(Method::POST, "/library/organize/apply", None).await.json(), json!({ "applied": 0 }));
}

#[tokio::test]
async fn library_organize_quarantines_instead_of_deleting() {
    let s = Server::new().await;
    let library = s.state.data_dir.join("library");
    let quarantine = s.state.data_dir.join("quarantine");
    std::fs::create_dir_all(&library).unwrap();
    // Unreadable files, which the planner quarantines as junk.
    let seed = |name: &str, bytes: &[u8]| {
        let hash = typeshelf_core::file_hash(bytes);
        std::fs::write(library.join(&hash), bytes).unwrap();
        s.state
            .db()
            .execute(
                "INSERT INTO library_files (hash, filename, size, ps_name, family, style, version, uploaded_at)
                 VALUES (?1, ?2, 1, ?2, ?2, 'Regular', '1.0', ?3)",
                [hash.clone(), name.to_string(), db::now()],
            )
            .unwrap();
        hash
    };
    let a = seed("Junk.ttf", b"junk one");
    let b = seed("Other.ttf", b"junk two");
    let missing = seed("Gone.ttf", b"junk three");
    std::fs::remove_file(library.join(&missing)).unwrap();
    std::fs::create_dir_all(&quarantine).unwrap();
    std::fs::write(quarantine.join("Other.ttf"), b"already here").unwrap();

    let r = s.admin(Method::POST, "/library/organize/apply", None).await;
    assert_eq!(r.json(), json!({ "applied": 2 }), "the file that cannot be moved is not applied");
    assert_eq!(std::fs::read(quarantine.join("Junk.ttf")).unwrap(), b"junk one");
    assert_eq!(std::fs::read(quarantine.join(format!("{}-Other.ttf", &b[..8]))).unwrap(), b"junk two");
    assert_eq!(std::fs::read(quarantine.join("Other.ttf")).unwrap(), b"already here");
    assert!(!library.join(&a).exists() && !library.join(&b).exists());
    let lib = s.admin(Method::GET, "/library", None).await.json();
    assert_eq!(lib["total"], 1);
    assert_eq!(lib["items"][0]["hash"], missing.as_str(), "a failed move keeps the row");
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
async fn settings_reject_out_of_range_days() {
    let s = Server::new().await;
    for bad in [json!({ "name_change_days": 0 }), json!({ "name_change_days": 3651 }), json!({ "name_change_days": 4_294_967_295u32 }),
                json!({ "timeline_retention_days": 0 }), json!({ "timeline_retention_days": 3651 })] {
        let r = s.admin(Method::PUT, "/settings", Some(bad.clone())).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}");
        assert_eq!(r.json()["error"], "invalid_setting", "{bad}");
    }
    let ok = s.admin(Method::PUT, "/settings", Some(json!({ "name_change_days": 3650, "timeline_retention_days": 3650 }))).await;
    assert_eq!(ok.status, StatusCode::OK);
    let ok = s.admin(Method::PUT, "/settings", Some(json!({ "name_change_days": 1, "timeline_retention_days": null }))).await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.json()["timeline_retention_days"], Value::Null);
}

#[tokio::test]
async fn name_changes_csv() {
    let s = Server::new().await;
    let token = s.register("inst-1", "Budi").await;
    s.desktop(Method::PUT, "/me/name", &token, Some(json!({ "name": "Santoso, Budi" }))).await;

    let page = s.admin(Method::GET, "/name-changes", None).await.json();
    assert_eq!(page["total"], 2);
    let rows = &page["items"];
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
    let second = s.admin(Method::GET, "/name-changes?offset=1&limit=1", None).await.json();
    assert_eq!(second["items"][0]["by"], "first");
    assert_eq!(second["total"], 2);
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
    assert_eq!(reports["items"][0]["duplicates"], 3);
    assert_eq!(reports["total"], 1);

    let page = s.send(Method::GET, "/installs/a", &[], None).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&page.body).contains("not built"));
}

#[tokio::test]
async fn admin_lists_are_paged() {
    let s = Server::new().await;
    for (id, name) in [("a", "Ana"), ("b", "Budi"), ("c", "Citra")] {
        s.register(id, name).await;
    }
    let first = s.admin(Method::GET, "/installs?limit=2", None).await.json();
    assert_eq!(first["total"], 3);
    let names: Vec<&str> = first["items"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Ana", "Budi"]);
    let second = s.admin(Method::GET, "/installs?offset=2&limit=2", None).await.json();
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_eq!(second["items"][0]["name"], "Citra");
    assert_eq!(s.admin(Method::GET, "/installs?q=bud&limit=2", None).await.json()["total"], 1);

    // 250 fonts: the default page is 50, the limit is capped at 200, and search/active filter in SQL.
    let token = s.register("d", "Dewi").await;
    let active: Vec<Value> = (0..250)
        .map(|i| json!({ "hash": format!("h{i:03}"), "ps_name": format!("Font{i:03}-Regular"), "family": format!("Font {i:03}"), "style": "Regular", "source": "user" }))
        .collect();
    assert_eq!(s.desktop(Method::PUT, "/me/fonts", &token, Some(json!({ "active": active }))).await.status, StatusCode::NO_CONTENT);
    let page = s.admin(Method::GET, "/installs/d/fonts", None).await.json();
    assert_eq!((page["total"].as_u64(), page["items"].as_array().unwrap().len()), (Some(250), 50));
    let capped = s.admin(Method::GET, "/installs/d/fonts?limit=1000", None).await.json();
    assert_eq!(capped["items"].as_array().unwrap().len(), 200);
    let last = s.admin(Method::GET, "/installs/d/fonts?offset=200&limit=200", None).await.json();
    assert_eq!(last["items"].as_array().unwrap().len(), 50);
    assert_eq!(last["items"][0]["family"], "Font 200");
    let found = s.admin(Method::GET, "/installs/d/fonts?q=font%2024", None).await.json();
    assert_eq!(found["total"], 10); // Font 240..249
    assert_eq!(s.admin(Method::GET, "/installs/d/fonts?active=false", None).await.json()["total"], 0);
    assert_eq!(s.admin(Method::GET, "/installs/x/fonts", None).await.status, StatusCode::NOT_FOUND);
}

fn request(name: &str, note: Option<&str>) -> Value {
    event(json!({ "kind": "font_requested", "name": name, "note": note }))
}

#[tokio::test]
async fn font_requests_reach_the_admin() {
    let s = Server::new().await;
    let ana = s.register("a", "Ana").await;
    let laptop = s.register("b", "Ana S.").await;
    assert_eq!(s.send(Method::GET, "/api/admin/requests", &[], None).await.status, StatusCode::UNAUTHORIZED);

    let r = s.desktop(Method::POST, "/me/events", &ana, Some(request("Söhne Buch", Some("<b>Acme</b> deck")))).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    // Asking again for the same font while it is open adds no second row.
    s.desktop(Method::POST, "/me/events", &ana, Some(request("söhne buch", None))).await;
    s.desktop(Method::POST, "/me/events", &laptop, Some(request("Inter", None))).await;
    // Text is cut to the limits; an empty name is ignored.
    s.desktop(Method::POST, "/me/events", &laptop, Some(request(&"x".repeat(500), Some(&"n".repeat(5000))))).await;
    assert_eq!(s.desktop(Method::POST, "/me/events", &laptop, Some(request("  ", None))).await.status, StatusCode::NO_CONTENT);

    let list = s.admin(Method::GET, "/requests", None).await.json();
    assert_eq!(list["total"], 3);
    let names: Vec<&str> = list["items"].as_array().unwrap().iter().map(|i| i["font_name"].as_str().unwrap()).collect();
    assert_eq!(names, ["x".repeat(200).as_str(), "Inter", "Söhne Buch"], "newest first");
    assert_eq!(list["items"][0]["note"].as_str().unwrap().chars().count(), 1000);
    let sohne = &list["items"][2];
    assert_eq!((sohne["install_id"].as_str(), sohne["install_name"].as_str()), (Some("a"), Some("Ana")));
    assert_eq!(sohne["note"], "<b>Acme</b> deck", "stored as typed; the UI renders it as text");
    assert!(sohne["done_at"].is_null());
    assert_eq!(s.admin(Method::GET, "/overview", None).await.json()["attention"]["requests"], 3);
    let timeline = s.admin(Method::GET, "/installs/a/timeline", None).await.json();
    let texts: Vec<&str> = timeline["items"].as_array().unwrap().iter().map(|i| i["text"].as_str().unwrap()).collect();
    assert_eq!(texts[..2], ["Asked for söhne buch", "Asked for Söhne Buch"], "a repeat still reaches the timeline");

    // After a merge the laptop's requests belong to the person.
    let merged = s.admin(Method::POST, "/installs/merge", Some(json!({ "primary": "a", "secondary": "b", "keep_name": "primary" }))).await;
    assert_eq!(merged.status, StatusCode::OK);
    let list = s.admin(Method::GET, "/requests", None).await.json();
    assert!(list["items"].as_array().unwrap().iter().all(|i| i["install_id"] == "a" && i["install_name"] == "Ana"));

    let id = sohne["id"].as_i64().unwrap();
    assert_eq!(s.admin(Method::POST, &format!("/requests/{id}/done"), None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(s.admin(Method::GET, "/requests?state=open", None).await.json()["total"], 2);
    let done = s.admin(Method::GET, "/requests?state=done", None).await.json();
    assert_eq!(done["total"], 1);
    assert!(done["items"][0]["done_at"].is_string());
    assert_eq!(s.admin(Method::GET, "/overview", None).await.json()["attention"]["requests"], 2);
    // Once done, asking again opens a new request.
    s.desktop(Method::POST, "/me/events", &ana, Some(request("Söhne Buch", None))).await;
    assert_eq!(s.admin(Method::GET, "/requests?state=open", None).await.json()["total"], 3);

    assert_eq!(s.admin(Method::POST, &format!("/requests/{id}/reopen"), None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(s.admin(Method::GET, "/requests?state=open&limit=2", None).await.json()["items"].as_array().unwrap().len(), 2);
    assert_eq!(s.admin(Method::GET, "/requests?state=nope", None).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(s.admin(Method::POST, "/requests/9999/done", None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(s.send(Method::POST, &format!("/api/admin/requests/{id}/done"), &[], None).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn font_requests_are_capped_per_person() {
    let s = Server::new().await;
    let token = s.register("a", "Ana").await;
    let events: Vec<Value> = (0..60).map(|i| json!({ "kind": "font_requested", "name": format!("Font {i}"), "at": Utc::now() })).collect();
    assert_eq!(s.desktop(Method::POST, "/me/events", &token, Some(json!({ "events": events }))).await.status, StatusCode::NO_CONTENT);
    assert_eq!(s.admin(Method::GET, "/requests", None).await.json()["total"], 50);
}

#[tokio::test]
async fn a_revoked_install_is_refused_hidden_and_kept_in_name_history() {
    let s = Server::new().await;
    let a = s.register("a", "Ana").await;
    let b = s.register("b", "Budi").await;
    let hash = s.library_font().await;
    s.admin(Method::POST, &format!("/library/{hash}/deliver"), Some(json!({ "target": ["a", "b"] }))).await;
    s.desktop(Method::POST, "/me/events", &a, Some(event(json!({ "kind": "font_activated", "font": font("h1"), "source": "user" })))).await;
    s.desktop(Method::POST, "/me/events", &a, Some(request("Söhne", None))).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = s.app.clone();
    tokio::spawn(async move { axum::serve(listener, router).await });
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/v1/me/ws?key={}&token={a}", s.key)).await.unwrap();
    for _ in 0..50 {
        if s.state.hub.is_connected("a") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    assert_eq!(s.admin(Method::DELETE, "/installs/a", None).await.status, StatusCode::NO_CONTENT);
    // The token stops working at once, with its own error code.
    let me = s.desktop(Method::GET, "/me", &a, None).await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);
    assert_eq!(me.json()["error"], "revoked");
    assert_eq!(s.desktop(Method::GET, "/me", &b, None).await.status, StatusCode::OK);
    // The open socket is closed.
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await.expect("the socket was not closed") {
            None | Some(Err(_)) | Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break,
            Some(Ok(_)) => {}
        }
    }
    assert!(!s.state.hub.is_connected("a"));
    // Gone from lists, counts and the admin's per-install endpoints.
    let list = s.admin(Method::GET, "/installs", None).await.json();
    assert_eq!(list["total"], 1);
    assert_eq!(list["items"][0]["id"], "b");
    assert_eq!(s.admin(Method::GET, "/overview", None).await.json()["installs"], 1);
    assert_eq!(s.admin(Method::GET, "/installs/a", None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(s.admin(Method::DELETE, "/installs/a", None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(s.admin(Method::DELETE, "/installs/nobody", None).await.status, StatusCode::NOT_FOUND);
    let lib = s.admin(Method::GET, "/library", None).await.json();
    assert_eq!(lib["items"][0]["delivered_to"], json!(["b"]), "its deliveries are dropped");
    // Broadcasts skip it.
    s.admin(Method::PUT, "/settings", Some(json!({ "name_change_days": 7 }))).await;
    let pending: i64 = s.state.db().query_row("SELECT COUNT(*) FROM commands WHERE install_id = 'a' AND acked_at IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(pending, 0);
    // Its name history is still exported.
    let csv = String::from_utf8(s.admin(Method::GET, "/name-changes.csv", None).await.body).unwrap();
    assert!(csv.lines().any(|l| l.contains(",a,,Ana,first,")), "{csv}");

    // Registering again with the same install id makes a new install; the old token stays refused.
    let again = s.register("a", "Ana Baru").await;
    assert_eq!(s.desktop(Method::GET, "/me", &again, None).await.json()["name"], "Ana Baru");
    assert_eq!(s.desktop(Method::GET, "/me", &a, None).await.json()["error"], "revoked");
    let d = s.admin(Method::GET, "/installs/a", None).await.json();
    assert_eq!(d["previous_names"], json!([]), "the new install does not inherit the old history");
    assert_eq!(d["active_fonts"], 0);
    assert_eq!(s.admin(Method::GET, "/installs", None).await.json()["total"], 2);
    let csv = String::from_utf8(s.admin(Method::GET, "/name-changes.csv", None).await.body).unwrap();
    assert_eq!(csv.lines().filter(|l| l.contains(",Ana,first,")).count(), 1, "{csv}");
    assert_eq!(csv.lines().filter(|l| l.contains(",Ana Baru,first,")).count(), 1, "{csv}");
    // Its font request moved with the old history, not to the new install.
    let owner: String = s.state.db().query_row("SELECT install_id FROM font_requests WHERE font_name = 'Söhne'", [], |r| r.get(0)).unwrap();
    assert!(owner.starts_with("a~revoked-"), "{owner}");
}

#[tokio::test]
async fn revoking_a_merged_primary_revokes_the_whole_group() {
    let s = Server::new().await;
    let old = s.register("old", "Budi").await;
    let new = s.register("new", "Budi S.").await;
    let other = s.register("other", "Citra").await;
    s.admin(Method::POST, "/installs/merge", Some(json!({ "primary": "old", "secondary": "new", "keep_name": "primary" }))).await;
    assert_eq!(s.admin(Method::DELETE, "/installs/old", None).await.status, StatusCode::NO_CONTENT);
    for token in [&old, &new] {
        assert_eq!(s.desktop(Method::GET, "/me", token, None).await.json()["error"], "revoked");
    }
    assert_eq!(s.desktop(Method::GET, "/me", &other, None).await.status, StatusCode::OK);
    assert_eq!(s.admin(Method::GET, "/installs", None).await.json()["total"], 1);
    let merge = s.admin(Method::POST, "/installs/merge", Some(json!({ "primary": "other", "secondary": "new", "keep_name": "primary" }))).await;
    assert_eq!(merge.status, StatusCode::NOT_FOUND, "a revoked install cannot be merged");
}

#[tokio::test]
async fn undelivering_changes_what_new_installs_receive() {
    let s = Server::new().await;
    let a = s.register("a", "Ana").await;
    let b = s.register("b", "Budi").await;
    let hash = s.library_font().await;
    let path = format!("/library/{hash}/deliver");
    let deliver = |target: Value| s.admin(Method::POST, &path, Some(json!({ "target": target })));
    assert_eq!(deliver(json!("all")).await.json()["delivered_to"], "all");
    // Narrowing "everyone" to one person replaces the targets.
    let r = deliver(json!(["a"])).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["delivered_to"], json!(["a"]));
    let late = s.register("late", "Dewi").await;
    assert!(s.commands(&late).await.iter().all(|c| c["kind"] != "deliver"), "a new install no longer gets it");
    assert_eq!(s.desktop(Method::GET, &format!("/library/files/{hash}"), &late, None).await.status, StatusCode::NOT_FOUND);
    assert!(s.desktop(Method::GET, "/me/library", &b, None).await.json().as_array().unwrap().is_empty());
    assert_eq!(s.desktop(Method::GET, "/me/library", &a, None).await.json().as_array().unwrap().len(), 1);
    // Unticking everyone leaves nobody.
    assert_eq!(deliver(json!([])).await.json()["delivered_to"], "none");
    assert!(s.desktop(Method::GET, "/me/library", &a, None).await.json().as_array().unwrap().is_empty());
    assert_eq!(deliver(json!(["nobody"])).await.status, StatusCode::NOT_FOUND);
}

impl Server {
    /// A library row with an (unparseable) stored file; returns its hash.
    fn seed_file(&self, name: &str, bytes: &[u8]) -> String {
        let library = self.state.data_dir.join("library");
        std::fs::create_dir_all(&library).unwrap();
        let hash = typeshelf_core::file_hash(bytes);
        std::fs::write(library.join(&hash), bytes).unwrap();
        self.state
            .db()
            .execute(
                "INSERT INTO library_files (hash, filename, size, ps_name, family, style, version, uploaded_at)
                 VALUES (?1, ?2, 1, ?2, ?2, 'Regular', '1.0', ?3)",
                [hash.clone(), name.to_string(), db::now()],
            )
            .unwrap();
        hash
    }
}

#[tokio::test]
async fn deleting_a_library_file_moves_it_to_quarantine() {
    let s = Server::new().await;
    let a = s.register("a", "Ana").await;
    let hash = s.seed_file("Gone.ttf", b"bytes of gone");
    s.admin(Method::POST, &format!("/library/{hash}/deliver"), Some(json!({ "target": "all" }))).await;
    assert_eq!(s.admin(Method::DELETE, &format!("/library/{hash}"), None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(s.admin(Method::GET, "/library", None).await.json()["total"], 0);
    assert!(!s.state.data_dir.join("library").join(&hash).exists());
    assert_eq!(std::fs::read(s.state.data_dir.join("quarantine").join("Gone.ttf")).unwrap(), b"bytes of gone");
    assert_eq!(s.desktop(Method::GET, &format!("/library/files/{hash}"), &a, None).await.status, StatusCode::NOT_FOUND);
    let deliveries: i64 = s.state.db().query_row("SELECT COUNT(*) FROM deliveries", [], |r| r.get(0)).unwrap();
    assert_eq!(deliveries, 0);
    assert_eq!(s.admin(Method::DELETE, &format!("/library/{hash}"), None).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn revoking_and_deleting_are_audited() {
    let s = Server::new().await;
    s.register("a", "Ana").await;
    let hash = s.seed_file("Gone.ttf", b"bytes of gone");
    assert_eq!(s.admin(Method::DELETE, "/installs/a", None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(s.admin(Method::DELETE, &format!("/library/{hash}"), None).await.status, StatusCode::NO_CONTENT);
    let revoked = s.audit("?action=install_revoked").await;
    assert_eq!(revoked["total"], 1);
    assert_eq!((revoked["items"][0]["admin"].as_str(), revoked["items"][0]["target"].as_str()), (Some("admin@example.com"), Some("a")));
    let deleted = s.audit("?action=font_deleted").await;
    assert_eq!(deleted["total"], 1);
    assert_eq!((deleted["items"][0]["target"].as_str(), deleted["items"][0]["detail"].as_str()), (Some(hash.as_str()), Some("Gone.ttf")));
}

#[tokio::test]
async fn bulk_library_actions_take_ids_or_the_filter() {
    let s = Server::new().await;
    let a = s.register("a", "Ana").await;
    let one = s.seed_file("Alpha One.ttf", b"alpha one");
    let two = s.seed_file("Alpha Two.ttf", b"alpha two");
    let other = s.seed_file("Beta.ttf", b"beta");
    let bulk = |body: Value| s.admin(Method::POST, "/library/bulk", Some(body));
    let blocked = || async {
        let lib = s.admin(Method::GET, "/library", None).await.json();
        lib["items"].as_array().unwrap().iter().filter(|f| f["blocked"] == true).map(|f| f["hash"].as_str().unwrap().to_string()).collect::<Vec<_>>()
    };

    // By ids.
    let r = bulk(json!({ "action": "block", "hashes": [one] })).await;
    assert_eq!(r.json(), json!({ "changed": 1, "skipped": 0, "failed": 0 }));
    assert_eq!(blocked().await, std::slice::from_ref(&one));
    // By the list's search filter, across every page: files already in that state are skipped.
    let r = bulk(json!({ "action": "block", "q": "alpha" })).await;
    assert_eq!(r.json(), json!({ "changed": 1, "skipped": 1, "failed": 0 }));
    assert_eq!(blocked().await.len(), 2);
    assert_eq!(bulk(json!({ "action": "unblock", "q": "alpha two" })).await.json(), json!({ "changed": 1, "skipped": 0, "failed": 0 }));
    // Deliver skips blocked files; an empty filter means the whole library.
    let r = bulk(json!({ "action": "deliver", "q": "", "target": ["a"], "activate": true })).await;
    assert_eq!(r.json(), json!({ "changed": 2, "skipped": 1, "failed": 0 }));
    assert_eq!(s.desktop(Method::GET, "/me/library", &a, None).await.json().as_array().unwrap().len(), 2);
    // Unknown hashes are skipped; delete quarantines.
    let r = bulk(json!({ "action": "delete", "hashes": [two, other, "nope"] })).await;
    assert_eq!(r.json(), json!({ "changed": 2, "skipped": 1, "failed": 0 }));
    assert_eq!(s.admin(Method::GET, "/library", None).await.json()["total"], 1);
    assert!(s.state.data_dir.join("quarantine").join("Beta.ttf").exists());

    assert_eq!(bulk(json!({ "action": "delete", "hashes": [one] })).await.json(), json!({ "changed": 0, "skipped": 1, "failed": 0 }), "blocked: skipped");
    assert_eq!(bulk(json!({ "action": "block" })).await.json()["error"], "no_selection");
    assert_eq!(bulk(json!({ "action": "deliver", "hashes": [one] })).await.json()["error"], "invalid_target");
}

#[tokio::test]
async fn a_blocked_file_cannot_be_deleted() {
    let s = Server::new().await;
    let hash = s.seed_file("Blocked.ttf", b"blocked bytes");
    s.admin(Method::POST, &format!("/library/{hash}/block"), Some(json!({ "blocked": true }))).await;
    let r = s.admin(Method::DELETE, &format!("/library/{hash}"), None).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.json()["error"], "blocked");
    assert_eq!(s.admin(Method::GET, "/library", None).await.json()["total"], 1);
    assert!(s.state.data_dir.join("library").join(&hash).exists());
    let free = s.seed_file("Free.ttf", b"free bytes");
    let r = s.admin(Method::POST, "/library/bulk", Some(json!({ "action": "delete", "hashes": [hash, free] }))).await;
    assert_eq!(r.json(), json!({ "changed": 1, "skipped": 1, "failed": 0 }));
    assert!(s.state.data_dir.join("library").join(&hash).exists());
}

#[tokio::test]
async fn the_same_file_can_be_deleted_again_and_again() {
    let s = Server::new().await;
    for _ in 0..3 {
        let hash = s.seed_file("Same.ttf", b"same bytes");
        assert_eq!(s.admin(Method::DELETE, &format!("/library/{hash}"), None).await.status, StatusCode::NO_CONTENT);
    }
    assert_eq!(std::fs::read_dir(s.state.data_dir.join("quarantine")).unwrap().count(), 3);
}

#[tokio::test]
async fn a_failing_file_does_not_hide_the_rest_of_a_bulk_delete() {
    let s = Server::new().await;
    let ok = s.seed_file("Fine.ttf", b"fine");
    // A file name no file system accepts: its move fails.
    let bad = s.seed_file(&format!("{}.ttf", "x".repeat(300)), b"too long");
    let r = s.admin(Method::POST, "/library/bulk", Some(json!({ "action": "delete", "hashes": [bad, ok] }))).await;
    assert_eq!(r.json(), json!({ "changed": 1, "skipped": 0, "failed": 1 }));
    let lib = s.admin(Method::GET, "/library", None).await.json();
    assert_eq!(lib["total"], 1);
    assert_eq!(lib["items"][0]["hash"], bad.as_str());
}

#[tokio::test]
async fn undelivering_drops_queued_deliveries() {
    let s = Server::new().await;
    let a = s.register("a", "Ana").await;
    let b = s.register("b", "Budi").await;
    let hash = s.library_font().await;
    s.admin(Method::POST, &format!("/library/{hash}/deliver"), Some(json!({ "target": ["a", "b"] }))).await;
    s.admin(Method::POST, &format!("/library/{hash}/deliver"), Some(json!({ "target": ["a"] }))).await;
    assert!(s.commands(&b).await.is_empty(), "b is offline and no longer a target");
    assert!(s.commands(&a).await.iter().any(|c| c["kind"] == "deliver"));
}

/* ---------- admin accounts, roles and the audit log ---------- */

impl Server {
    /// Signs in and returns the session cookie (`ts_session=...`).
    async fn sign_in(&self, email: &str, password: &str) -> String {
        let r = self.send(Method::POST, "/api/admin/login", &[], Some(json!({ "email": email, "password": password }))).await;
        assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&r.body));
        r.headers[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_string()
    }

    async fn as_admin(&self, cookie: &str, method: Method, path: &str, body: Option<Value>) -> Reply {
        self.send(method, &format!("/api/admin{path}"), &[("Cookie", cookie)], body).await
    }

    /// Adds a plain admin (as the owner) and returns their id and a session cookie.
    async fn plain_admin(&self, email: &str) -> (i64, String) {
        let r = self.admin(Method::POST, "/admins", Some(json!({ "email": email, "password": "password-two" }))).await;
        assert_eq!(r.status, StatusCode::CREATED);
        let id = self.admin_id(email).await;
        (id, self.sign_in(email, "password-two").await)
    }

    async fn admin_id(&self, email: &str) -> i64 {
        let list = self.admin(Method::GET, "/admins", None).await.json();
        list.as_array().unwrap().iter().find(|a| a["email"] == email).unwrap()["id"].as_i64().unwrap()
    }

    async fn audit(&self, query: &str) -> Value {
        let r = self.admin(Method::GET, &format!("/audit{query}"), None).await;
        assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.body));
        r.json()
    }
}

fn actions(page: &Value) -> Vec<String> {
    page["items"].as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn migration_makes_the_earliest_admin_the_owner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    {
        // A database from before roles were checked: everyone is "admin".
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(include_str!("../migrations/0001_init.sql")).unwrap();
        c.execute_batch(include_str!("../migrations/0002_command_fanout.sql")).unwrap();
        c.pragma_update(None, "user_version", 2).unwrap();
        for email in ["first@x.com", "second@x.com"] {
            admin::insert_admin(&c, email, "hash", "admin").unwrap();
        }
    }
    let c = db::open(Some(&path)).unwrap();
    let roles: Vec<(String, String)> = c
        .prepare("SELECT email, role FROM admins ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let want = [("first@x.com", "owner"), ("second@x.com", "admin")].map(|(e, r)| (e.to_string(), r.to_string()));
    assert_eq!(roles, want);
}

#[tokio::test]
async fn plain_admins_get_403_on_owner_only_routes() {
    let s = Server::new().await;
    let (two, cookie) = s.plain_admin("two@example.com").await;
    let owner = s.admin_id("admin@example.com").await;
    let forbidden = [
        (Method::POST, "/admins".to_string(), Some(json!({ "email": "three@example.com", "password": "longenough" }))),
        (Method::DELETE, format!("/admins/{owner}"), None),
        (Method::PUT, format!("/admins/{two}/role"), Some(json!({ "role": "owner" }))),
        (Method::POST, format!("/admins/{owner}/password"), Some(json!({ "password": "taken-over" }))),
        (Method::POST, "/settings/install-key".to_string(), None),
    ];
    for (method, path, body) in forbidden {
        let r = s.as_admin(&cookie, method.clone(), &path, body).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(r.json()["error"], "owner_only", "{method} {path}");
    }
    // Nothing changed, and the plain admin can still do everything else.
    assert_eq!(s.admin(Method::GET, "/me", None).await.json()["role"], "owner");
    assert_eq!(s.as_admin(&cookie, Method::GET, "/me", None).await.json(), json!({ "email": "two@example.com", "role": "admin" }));
    assert_eq!(s.as_admin(&cookie, Method::PUT, "/settings", Some(json!({ "name_change_days": 14 }))).await.status, StatusCode::OK);
    assert_eq!(s.as_admin(&cookie, Method::GET, "/audit", None).await.status, StatusCode::OK);
    assert_eq!(s.as_admin(&cookie, Method::GET, "/admins", None).await.status, StatusCode::OK);
}

#[tokio::test]
async fn owners_manage_admins_and_the_last_owner_stays() {
    let s = Server::new().await;
    let owner = s.admin_id("admin@example.com").await;
    let (two, cookie) = s.plain_admin("two@example.com").await;

    // The only owner can be neither demoted nor removed.
    for (method, path, body) in [(Method::PUT, format!("/admins/{owner}/role"), Some(json!({ "role": "admin" }))), (Method::DELETE, format!("/admins/{owner}"), None)] {
        let r = s.admin(method, &path, body).await;
        assert_eq!(r.status, StatusCode::CONFLICT, "{path}");
        assert_eq!(r.json()["error"], "last_owner");
    }
    assert_eq!(s.admin(Method::PUT, &format!("/admins/{two}/role"), Some(json!({ "role": "boss" }))).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(s.admin(Method::DELETE, "/admins/999", None).await.status, StatusCode::NOT_FOUND);

    // A role change applies to the next request of a session that is already signed in.
    assert_eq!(s.admin(Method::PUT, &format!("/admins/{two}/role"), Some(json!({ "role": "owner" }))).await.status, StatusCode::NO_CONTENT);
    assert_eq!(s.as_admin(&cookie, Method::GET, "/me", None).await.json()["role"], "owner");
    // With a second owner, the first can step down; then they are a plain admin.
    assert_eq!(s.admin(Method::PUT, &format!("/admins/{owner}/role"), Some(json!({ "role": "admin" }))).await.status, StatusCode::NO_CONTENT);
    assert_eq!(s.admin(Method::POST, "/settings/install-key", None).await.status, StatusCode::FORBIDDEN);

    // Removing an admin ends their sessions at once.
    assert_eq!(s.as_admin(&cookie, Method::DELETE, &format!("/admins/{owner}"), None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(s.admin(Method::GET, "/me", None).await.status, StatusCode::UNAUTHORIZED);
    let left = s.as_admin(&cookie, Method::GET, "/admins", None).await.json();
    assert_eq!(left.as_array().unwrap().len(), 1);
    let r = s.send(Method::POST, "/api/admin/login", &[], Some(json!({ "email": "admin@example.com", "password": "hunter2hunter2" }))).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn password_changes_end_sessions() {
    let s = Server::new().await;
    let (two, laptop) = s.plain_admin("two@example.com").await;
    let phone = s.sign_in("two@example.com", "password-two").await;

    // Changing your own password needs the current one.
    let wrong = s.as_admin(&laptop, Method::PUT, "/me/password", Some(json!({ "current": "nope", "password": "a-new-password" }))).await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    assert_eq!(wrong.json()["error"], "wrong_password");
    let weak = s.as_admin(&laptop, Method::PUT, "/me/password", Some(json!({ "current": "password-two", "password": "short" }))).await;
    assert_eq!(weak.json()["error"], "weak_password");
    let ok = s.as_admin(&laptop, Method::PUT, "/me/password", Some(json!({ "current": "password-two", "password": "a-new-password" }))).await;
    assert_eq!(ok.status, StatusCode::NO_CONTENT);
    // Every old session ends, the one that made the change included; it gets a fresh cookie instead.
    let fresh = ok.headers[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_string();
    assert_eq!(s.as_admin(&laptop, Method::GET, "/me", None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(s.as_admin(&phone, Method::GET, "/me", None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(s.as_admin(&fresh, Method::GET, "/me", None).await.status, StatusCode::OK);
    s.sign_in("two@example.com", "a-new-password").await;

    // An owner resets another admin's password: that admin's sessions end too.
    assert_eq!(s.admin(Method::POST, &format!("/admins/{two}/password"), Some(json!({ "password": "x" }))).await.json()["error"], "weak_password");
    assert_eq!(s.admin(Method::POST, &format!("/admins/{two}/password"), Some(json!({ "password": "reset-by-owner" }))).await.status, StatusCode::NO_CONTENT);
    assert_eq!(s.as_admin(&fresh, Method::GET, "/me", None).await.status, StatusCode::UNAUTHORIZED);
    s.sign_in("two@example.com", "reset-by-owner").await;
    // Owners change their own password with the current one, not by reset.
    let owner = s.admin_id("admin@example.com").await;
    let own = s.admin(Method::POST, &format!("/admins/{owner}/password"), Some(json!({ "password": "no-current-needed" }))).await;
    assert_eq!(own.status, StatusCode::BAD_REQUEST);
    assert_eq!(s.admin(Method::GET, "/me", None).await.status, StatusCode::OK);

    // The command-line recovery path (create-admin on an existing email) ends sessions the same way.
    let id = s.admin_id("two@example.com").await;
    let cookie = s.sign_in("two@example.com", "reset-by-owner").await;
    admin::set_password(&s.state.db(), id, &auth::hash_password("from-the-cli")).unwrap();
    assert_eq!(s.as_admin(&cookie, Method::GET, "/me", None).await.status, StatusCode::UNAUTHORIZED);
    s.sign_in("two@example.com", "from-the-cli").await;
}

#[tokio::test]
async fn the_audit_log_records_admin_actions_without_secrets() {
    let s = Server::new().await;
    s.register("inst-1", "Budi").await;
    s.register("inst-2", "Budi S.").await; // before the install key changes below
    let hash = s.library_font().await;
    s.admin(Method::PUT, "/settings", Some(json!({ "name_change_days": 9 }))).await;
    let key = s.admin(Method::POST, "/settings/install-key", None).await.json()["install_key"].as_str().unwrap().to_string();
    let (two, cookie) = s.plain_admin("two@example.com").await;
    s.admin(Method::PUT, &format!("/admins/{two}/role"), Some(json!({ "role": "owner" }))).await;
    s.admin(Method::PUT, &format!("/admins/{two}/role"), Some(json!({ "role": "admin" }))).await;
    s.admin(Method::POST, &format!("/admins/{two}/password"), Some(json!({ "password": "secret-reset-pw" }))).await;
    s.as_admin(&cookie, Method::GET, "/me", None).await; // ended by the reset
    let cookie = s.sign_in("two@example.com", "secret-reset-pw").await;
    s.as_admin(&cookie, Method::PUT, "/me/password", Some(json!({ "current": "secret-reset-pw", "password": "secret-own-pw" }))).await;
    s.admin(Method::POST, &format!("/library/{hash}/block"), Some(json!({ "blocked": true }))).await;
    s.admin(Method::POST, &format!("/library/{hash}/block"), Some(json!({ "blocked": false }))).await;
    s.admin(Method::POST, &format!("/library/{hash}/deliver"), Some(json!({ "target": ["inst-1"] }))).await;
    s.admin(Method::PUT, &format!("/library/{hash}"), Some(json!({ "license": "client" }))).await;
    s.admin(Method::POST, "/installs/inst-1/fonts", Some(json!({ "hash": hash, "active": true }))).await;
    s.admin(Method::POST, "/installs/inst-1/reset-name-limit", None).await;
    s.admin(Method::POST, "/installs/inst-1/organize-request", None).await;
    s.admin(Method::POST, "/installs/merge", Some(json!({ "primary": "inst-1", "secondary": "inst-2", "keep_name": "primary" }))).await;
    s.admin(Method::POST, "/installs/inst-2/split", None).await;
    s.admin(Method::POST, "/library/organize/apply", None).await;
    s.admin(Method::DELETE, &format!("/admins/{two}"), None).await;

    let all = s.audit("?limit=200").await;
    let mut got = actions(&all);
    got.reverse();
    let mut want = vec!["sign_in"]; // Server::new
    if got.contains(&"font_uploaded".to_string()) {
        want.push("font_uploaded");
    }
    want.extend([
        "settings_changed", "install_key_regenerated", "admin_added", "sign_in", "role_changed", "role_changed", "password_reset",
        "sign_in", "password_changed", "font_blocked", "font_unblocked", "font_delivered", "license_changed", "font_on",
        "name_limit_reset", "organize_requested", "installs_merged", "install_split", "organize_applied", "admin_removed",
    ]);
    assert_eq!(got, want);
    assert_eq!(all["total"], want.len());
    let first = &all["items"][0];
    assert_eq!(first["admin"], "admin@example.com");
    assert_eq!(first["target"], "two@example.com");
    assert!(first["at"].is_string());
    let settings = all["items"].as_array().unwrap().iter().find(|e| e["action"] == "settings_changed").unwrap();
    assert!(settings["detail"].as_str().unwrap().contains("name_change_days"), "{settings}");

    // No password, token or install key is ever stored.
    let text = all.to_string();
    for secret in [key.as_str(), s.key.as_str(), "secret-reset-pw", "secret-own-pw", "password-two", "hunter2hunter2"] {
        assert!(!text.contains(secret), "the audit log contains {secret}");
    }
    let cookie_value = s.cookie.split('=').nth(1).unwrap();
    assert!(!text.contains(cookie_value));

    // Filters and paging; the filter lists come with every page.
    let by_two = s.audit("?admin=TWO@example.com").await;
    assert_eq!(actions(&by_two), ["password_changed", "sign_in", "sign_in"]);
    let roles = s.audit("?action=role_changed&limit=1").await;
    assert_eq!((roles["total"].as_i64(), roles["items"].as_array().unwrap().len()), (Some(2), 1));
    assert_eq!(roles["items"][0]["detail"], "owner → admin");
    assert!(all["admins"].as_array().unwrap().contains(&json!("two@example.com")));
    assert!(all["actions"].as_array().unwrap().contains(&json!("install_key_regenerated")));
}

#[tokio::test]
async fn failed_sign_ins_are_audited_once_throttling_starts() {
    let s = Server::new().await;
    let login = |email: &'static str| s.send(Method::POST, "/api/admin/login", &[], Some(json!({ "email": email, "password": "wrong" })));
    for _ in 0..auth::LOGIN_MAX_FAILURES - 1 {
        login("admin@example.com").await;
    }
    assert_eq!(s.audit("?action=sign_in_failed").await["total"], 0, "ordinary typos are not logged");
    login("admin@example.com").await; // this one starts the lock
    for _ in 0..20 {
        assert_eq!(login("admin@example.com").await.status, StatusCode::TOO_MANY_REQUESTS);
    }
    let failed = s.audit("?action=sign_in_failed").await;
    assert_eq!(failed["total"], 1, "one row per lock, however many attempts follow");
    assert_eq!(failed["items"][0]["admin"], "admin@example.com");
    assert!(!failed.to_string().contains("wrong"));
    // Emails that are not admins never reach the log, so a flood of made-up emails cannot fill it.
    for _ in 0..auth::LOGIN_MAX_FAILURES + 2 {
        login("nobody@example.com").await;
    }
    assert_eq!(s.audit("?action=sign_in_failed").await["total"], 1);
}

impl Server {
    /// `POST /api/admin/update` with the given multipart fields `(name, bytes)`.
    async fn upload_update(&self, fields: &[(&str, &[u8])], cookie: &str) -> Reply {
        let boundary = "XUPDATEBOUNDARYX";
        let mut body = Vec::new();
        for (name, bytes) in fields {
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
            body.extend_from_slice(bytes);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        let req = Request::post("/api/admin/update")
            .header("Cookie", cookie)
            .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(body))
            .unwrap();
        self.raw(req).await
    }
}

/// A setup program, its manifest and signature, signed with a key made here; returns the public key too.
fn signed_release(version: &str, file: &[u8]) -> (String, String, [u8; 32]) {
    use typeshelf_core::update::{self, Manifest, Version};
    let (private, public) = update::generate_key().unwrap();
    let manifest = Manifest::for_bytes(Version::parse(version).unwrap(), file).to_text();
    let (signature, _) = update::sign(&private, manifest.as_bytes()).unwrap();
    (manifest, signature, update::public_key(Some(&public)).unwrap())
}

#[tokio::test]
async fn update_release_is_stored_as_uploaded_and_served_to_installs() {
    let s = Server::new().await;
    let token = s.register("a", "Ana").await;
    assert_eq!(s.admin(Method::GET, "/update", None).await.json(), Value::Null);
    assert_eq!(s.desktop(Method::GET, "/update", &token, None).await.status, StatusCode::NOT_FOUND);

    let file = b"MZ pretend setup program".repeat(1000);
    let (manifest, signature, key) = signed_release("0.2.0", &file);
    let fields: [(&str, &[u8]); 3] = [("file", &file), ("manifest", manifest.as_bytes()), ("signature", signature.as_bytes())];
    assert_eq!(s.upload_update(&fields, "").await.status, StatusCode::UNAUTHORIZED);
    // Only an owner uploads; a plain admin can still see the current release.
    let (_, plain) = s.plain_admin("two@example.com").await;
    let r = s.upload_update(&fields, &plain).await;
    assert_eq!((r.status, r.json()["error"].as_str()), (StatusCode::FORBIDDEN, Some("owner_only")));
    assert_eq!(s.as_admin(&plain, Method::GET, "/update", None).await.json(), Value::Null);
    let r = s.upload_update(&fields, &s.cookie).await;
    assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(r.json(), json!({ "version": "0.2.0", "size": file.len() }));
    assert_eq!(s.admin(Method::GET, "/update", None).await.json(), json!({ "version": "0.2.0", "size": file.len() }));
    let logged = s.audit("?action=release_uploaded").await;
    assert_eq!(logged["total"], 1);
    assert_eq!((logged["items"][0]["admin"].as_str(), logged["items"][0]["target"].as_str(), logged["items"][0]["detail"].as_str()), (Some("admin@example.com"), Some("0.2.0"), Some("")));

    // Installs get the manifest and signature byte for byte, and they verify with the key.
    let got = s.desktop(Method::GET, "/update", &token, None).await;
    assert_eq!(got.status, StatusCode::OK);
    let body = got.json();
    assert_eq!(body["manifest"], manifest.as_str());
    assert_eq!(body["signature"], signature.as_str());
    let m = typeshelf_core::update::verify_manifest(&key, body["manifest"].as_str().unwrap().as_bytes(), body["signature"].as_str().unwrap()).unwrap();
    let bin = s.desktop(Method::GET, "/update/file", &token, None).await;
    assert_eq!(bin.status, StatusCode::OK);
    assert_eq!(bin.body, file);
    assert_eq!(typeshelf_core::update::check_file(&m, &bin.body[..]), Ok(()));

    // Only authenticated installs.
    assert_eq!(s.send(Method::GET, "/api/v1/update", &[("X-Install-Key", &s.key)], None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(s.send(Method::GET, "/api/v1/update/file", &[("X-Install-Key", &s.key)], None).await.status, StatusCode::UNAUTHORIZED);

    // A newer upload replaces the current release.
    let newer = b"MZ newer".to_vec();
    let (manifest, signature, _) = signed_release("0.3.0", &newer);
    let r = s.upload_update(&[("file", &newer), ("manifest", manifest.as_bytes()), ("signature", signature.as_bytes())], &s.cookie).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(s.desktop(Method::GET, "/update/file", &token, None).await.body, newer);
    assert_eq!(s.desktop(Method::GET, "/update", &token, None).await.json()["manifest"], manifest.as_str());
}

#[tokio::test]
async fn update_upload_refuses_a_file_that_does_not_match_its_manifest() {
    let s = Server::new().await;
    let token = s.register("a", "Ana").await;
    let file = b"MZ pretend setup program".to_vec();
    let (manifest, signature, _) = signed_release("0.2.0", &file);
    let mut other = file.clone();
    other[3] ^= 1;
    let longer = [&file[..], b"!"].concat();
    let bad_manifest = manifest.replace("version 0.2.0", "version 0.2");
    for (fields, code) in [
        (vec![("file", &other[..]), ("manifest", manifest.as_bytes()), ("signature", signature.as_bytes())], "file_mismatch"),
        (vec![("file", &longer[..]), ("manifest", manifest.as_bytes()), ("signature", signature.as_bytes())], "file_mismatch"),
        (vec![("file", &file[..]), ("manifest", bad_manifest.as_bytes()), ("signature", signature.as_bytes())], "bad_manifest"),
        (vec![("file", &file[..]), ("manifest", manifest.as_bytes()), ("signature", b"not hex".as_slice())], "bad_signature"),
        (vec![("file", &file[..]), ("manifest", manifest.as_bytes())], "missing_field"),
    ] {
        let r = s.upload_update(&fields, &s.cookie).await;
        assert_eq!((r.status, r.json()["error"].as_str()), (StatusCode::BAD_REQUEST, Some(code)), "{}", String::from_utf8_lossy(&r.body));
    }
    assert_eq!(s.admin(Method::GET, "/update", None).await.json(), Value::Null);
    assert_eq!(s.desktop(Method::GET, "/update", &token, None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(s.desktop(Method::GET, "/update/file", &token, None).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn update_upload_over_the_cap_is_413() {
    let s = Server::new().await;
    let file = vec![0u8; typeshelf_server::update::MAX_UPLOAD + 1];
    let r = s.upload_update(&[("file", &file), ("manifest", b"m"), ("signature", b"s")], &s.cookie).await;
    assert_eq!((r.status, r.json()["error"].as_str()), (StatusCode::PAYLOAD_TOO_LARGE, Some("bad_upload")));
}
