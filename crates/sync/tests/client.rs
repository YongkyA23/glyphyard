//! Exercises SyncClient against a small in-process mock of the Desktop API.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as UrlPath, State};
use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use typeshelf_core::organizer::{merge, OrganizerDoc, SyncSet};
use typeshelf_protocol::*;
use typeshelf_sync::{CancellationToken, Config, Organizer, Status, SyncClient, SyncError};

const KEY: &str = "install-key";
const TOKEN: &str = "install-token";

#[derive(Default)]
struct Mock {
    events: Mutex<Vec<TimedEvent>>,
    acks: Mutex<Vec<(i64, bool)>>,
    pending: Mutex<Vec<Command>>,
    ws_connects: AtomicUsize,
    /// Once set, events with changes 500 are accepted.
    server_healed: AtomicBool,
    /// `(rev, doc)` stored for `/me/organizer`; `None` answers 404 like an older server.
    organizer: Mutex<Option<(i64, OrganizerDoc)>>,
    /// Pushed by "another computer" just before the next PUT is handled, so that PUT conflicts.
    organizer_sneak: Mutex<Option<OrganizerDoc>>,
    /// Every PUT's `(base_rev, doc)`.
    organizer_puts: Mutex<Vec<(i64, OrganizerDoc)>>,
}

type S = State<Arc<Mock>>;

async fn get_organizer(State(m): S, h: HeaderMap) -> Response {
    if let Err(s) = auth(&h, true) {
        return s.into_response();
    }
    match m.organizer.lock().unwrap().clone() {
        Some((rev, doc)) => Json(json!({ "rev": rev, "doc": doc })).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Like the server: merges what is stored with what is sent, 409 with the current one on a stale base_rev.
async fn put_organizer(State(m): S, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if let Err(s) = auth(&h, true) {
        return s.into_response();
    }
    let (base, doc): (i64, OrganizerDoc) = (body["base_rev"].as_i64().unwrap(), serde_json::from_value(body["doc"].clone()).unwrap());
    m.organizer_puts.lock().unwrap().push((base, doc.clone()));
    let mut stored = m.organizer.lock().unwrap();
    let (rev, cur) = stored.clone().expect("PUT only after GET");
    if let Some(other) = m.organizer_sneak.lock().unwrap().take() {
        *stored = Some((rev + 1, merge(&cur, &other)));
    }
    let (rev, cur) = stored.clone().unwrap();
    if base != rev {
        return (StatusCode::CONFLICT, Json(json!({ "error": "organizer_conflict", "message": "", "rev": rev, "doc": cur }))).into_response();
    }
    let next = (rev + 1, merge(&cur, &doc));
    *stored = Some(next.clone());
    Json(json!({ "rev": next.0, "doc": next.1 })).into_response()
}

fn auth(h: &HeaderMap, bearer: bool) -> Result<(), StatusCode> {
    if h.get(INSTALL_KEY_HEADER).and_then(|v| v.to_str().ok()) != Some(KEY) {
        return Err(StatusCode::FORBIDDEN);
    }
    let expected = format!("Bearer {TOKEN}");
    if bearer && h.get(AUTHORIZATION).and_then(|v| v.to_str().ok()) != Some(&expected) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

fn policy() -> Policy {
    Policy { name_change_days: 30, allow_connection_edit: true, notify_admin_changes: false, brand_name: String::new(), brand_color: String::new(), brand_logo: String::new(), connection_password_hash: String::new() }
}

async fn health(h: HeaderMap) -> StatusCode {
    auth(&h, false).err().unwrap_or(StatusCode::NO_CONTENT)
}

async fn register(h: HeaderMap, Json(req): Json<RegisterRequest>) -> Result<Json<RegisterResponse>, StatusCode> {
    auth(&h, false)?;
    Ok(Json(RegisterResponse { token: TOKEN.into(), name: req.name, next_name_change: None, policy: policy() }))
}

async fn me(h: HeaderMap) -> Result<Json<Value>, StatusCode> {
    auth(&h, true)?;
    Ok(Json(json!({ "name": "Budi", "next_name_change": null, "policy": policy() })))
}

async fn change_name(h: HeaderMap, Json(_): Json<NameChange>) -> Result<Response, StatusCode> {
    auth(&h, true)?;
    let body = json!({ "error": "name_locked", "next_name_change": "2026-11-01T00:00:00Z" });
    Ok((StatusCode::CONFLICT, Json(body)).into_response())
}

async fn events(State(m): S, h: HeaderMap, Json(batch): Json<EventBatch>) -> StatusCode {
    if let Err(s) = auth(&h, true) {
        return s;
    }
    // Events with changes 400 / 500 make the mock refuse the whole batch with that status.
    for e in &batch.events {
        match e.event {
            Event::OrganizeApplied { changes: 400 } => return StatusCode::BAD_REQUEST,
            Event::OrganizeApplied { changes: 500 } if !m.server_healed.load(Ordering::SeqCst) => return StatusCode::INTERNAL_SERVER_ERROR,
            _ => {}
        }
    }
    m.events.lock().unwrap().extend(batch.events);
    StatusCode::NO_CONTENT
}

async fn commands(State(m): S, h: HeaderMap) -> Result<Json<Vec<Command>>, StatusCode> {
    auth(&h, true)?;
    Ok(Json(m.pending.lock().unwrap().clone()))
}

fn record_ack(m: &Mock, id: i64, ok: bool) {
    m.acks.lock().unwrap().push((id, ok));
    m.pending.lock().unwrap().retain(|c| c.id != id);
}

async fn ack(State(m): S, UrlPath(id): UrlPath<i64>, h: HeaderMap, Json(body): Json<Value>) -> StatusCode {
    if let Err(s) = auth(&h, true) {
        return s;
    }
    record_ack(&m, id, body["ok"] == true);
    StatusCode::NO_CONTENT
}

async fn ws(State(m): S, h: HeaderMap, upgrade: WebSocketUpgrade) -> Response {
    match auth(&h, true) {
        Ok(()) => upgrade.on_upgrade(move |socket| ws_session(m, socket)),
        Err(s) => s.into_response(),
    }
}

/// Pushes command 100+n on the n-th connection; drops the first connection once that one is acked.
async fn ws_session(m: Arc<Mock>, mut socket: WebSocket) {
    let n = m.ws_connects.fetch_add(1, Ordering::SeqCst) + 1;
    let pushed = 100 + n as i64;
    let cmd = ServerMessage::Command(Command { id: pushed, kind: CommandKind::OrganizeRequest });
    let text = serde_json::to_string(&cmd).unwrap();
    if socket.send(Message::Text(text.into())).await.is_err() {
        return;
    }
    let identity = ServerMessage::Identity { name: format!("Budi {n}"), next_name_change: None };
    if socket.send(Message::Text(serde_json::to_string(&identity).unwrap().into())).await.is_err() {
        return;
    }
    while let Some(Ok(msg)) = socket.recv().await {
        let Message::Text(text) = msg else { continue };
        let Ok(ClientMessage::Ack { id, ok, .. }) = serde_json::from_str(&text) else { continue };
        record_ack(&m, id, ok);
        if n == 1 && id == pushed {
            return;
        }
    }
}

async fn serve(mock: Arc<Mock>) -> String {
    let app = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/installs", post(register))
        .route("/api/v1/me", get(me))
        .route("/api/v1/me/name", put(change_name))
        .route("/api/v1/me/events", post(events))
        .route("/api/v1/me/commands", get(commands))
        .route("/api/v1/me/commands/{id}/ack", post(ack))
        .route("/api/v1/me/ws", get(ws))
        .route("/api/v1/me/organizer", get(get_organizer).put(put_organizer))
        .with_state(mock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

/// A URL nothing listens on.
async fn dead_url() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    format!("http://{}", listener.local_addr().unwrap())
}

fn config(url: &str, token: Option<&str>, outbox: &Path) -> Config {
    Config {
        server_url: url.into(),
        install_key: KEY.into(),
        token: token.map(Into::into),
        outbox_path: outbox.into(),
    }
}

fn event(changes: u32) -> TimedEvent {
    TimedEvent { at: Utc::now(), event: Event::OrganizeApplied { changes } }
}

async fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..500 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn register_flow_and_name_lock() {
    let url = serve(Arc::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let client = SyncClient::new(config(&url, None, &dir.path().join("outbox.jsonl")));

    client.health().await.unwrap();
    let wrong_key = SyncClient::new(Config { install_key: "nope".into(), ..config(&url, None, dir.path()) });
    assert!(matches!(wrong_key.health().await, Err(SyncError::Server { status: 403, .. })));
    let unreachable = SyncClient::new(config(&dead_url().await, None, dir.path()));
    assert!(matches!(unreachable.health().await, Err(SyncError::Offline)));

    assert!(matches!(client.me().await, Err(SyncError::Server { status: 401, .. })));
    let req = RegisterRequest {
        install_id: "id-1".into(),
        name: "Budi".into(),
        os: "windows".into(),
        app_version: "0.1.0".into(),
    };
    let res = client.register(req).await.unwrap();
    assert_eq!(res.token, TOKEN);
    assert_eq!(client.me().await.unwrap().name, "Budi");

    match client.change_name("Budi Santoso").await {
        Err(SyncError::NameLocked { next_name_change }) => {
            assert_eq!(next_name_change, Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap())
        }
        other => panic!("expected NameLocked, got {other:?}"),
    }
}

#[tokio::test]
async fn outbox_survives_restart_and_flushes_on_connect() {
    let dir = tempfile::tempdir().unwrap();
    let outbox = dir.path().join("outbox.jsonl");

    let offline = SyncClient::new(config(&dead_url().await, Some(TOKEN), &outbox));
    offline.queue_events(vec![event(1), event(2)]).await.unwrap();
    assert!(matches!(offline.flush().await, Err(SyncError::Offline)));
    drop(offline);

    let mock = Arc::new(Mock::default());
    let url = serve(mock.clone()).await;
    let client = Arc::new(SyncClient::new(config(&url, Some(TOKEN), &outbox)));
    let cancel = CancellationToken::new();
    let task = tokio::spawn({
        let (client, cancel) = (client.clone(), cancel.clone());
        async move { client.run(cancel, |_| async { Ok(()) }).await }
    });

    wait_until("outbox flush on connect", || mock.events.lock().unwrap().len() == 2).await;
    client.status().wait_for(|s| *s == Status::Connected).await.unwrap();
    client.queue_events(vec![event(3)]).await.unwrap();
    wait_until("flush while connected", || mock.events.lock().unwrap().len() == 3).await;
    wait_until("outbox emptied", || std::fs::read_to_string(&outbox).unwrap().trim().is_empty()).await;

    let sent: Vec<_> = mock.events.lock().unwrap().iter().map(|e| e.event.clone()).collect();
    let expected: Vec<_> = (1..=3).map(|changes| Event::OrganizeApplied { changes }).collect();
    assert_eq!(sent, expected);

    cancel.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn rejected_batch_is_dropped_and_later_events_are_delivered() {
    let mock = Arc::new(Mock::default());
    let url = serve(mock.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let outbox = dir.path().join("outbox.jsonl");
    let client = SyncClient::new(config(&url, Some(TOKEN), &outbox));

    // A full first batch (200) the server answers with 400, then one valid event behind it.
    let mut events = vec![event(400); 200];
    events.push(event(7));
    client.queue_events(events).await.unwrap();
    client.flush().await.unwrap();

    let sent: Vec<_> = mock.events.lock().unwrap().iter().map(|e| e.event.clone()).collect();
    assert_eq!(sent, [Event::OrganizeApplied { changes: 7 }]);
    assert!(std::fs::read_to_string(&outbox).unwrap().trim().is_empty());
}

#[tokio::test]
async fn server_error_keeps_the_batch_in_the_outbox() {
    let mock = Arc::new(Mock::default());
    let url = serve(mock.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let outbox = dir.path().join("outbox.jsonl");
    let client = SyncClient::new(config(&url, Some(TOKEN), &outbox));

    client.queue_events(vec![event(500)]).await.unwrap();
    assert!(matches!(client.flush().await, Err(SyncError::Server { status: 500, .. })));
    assert_eq!(std::fs::read_to_string(&outbox).unwrap().lines().count(), 1);
    assert!(mock.events.lock().unwrap().is_empty());
}

fn kinds(events: &[TimedEvent]) -> Vec<Event> {
    events.iter().map(|e| e.event.clone()).collect()
}

fn changes(ns: &[u32]) -> Vec<Event> {
    ns.iter().map(|&changes| Event::OrganizeApplied { changes }).collect()
}

#[tokio::test]
async fn only_the_rejected_event_is_dropped_from_a_batch() {
    let mock = Arc::new(Mock::default());
    let url = serve(mock.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let outbox = dir.path().join("outbox.jsonl");
    let client = SyncClient::new(config(&url, Some(TOKEN), &outbox));

    client.queue_events(vec![event(1), event(2), event(400), event(3)]).await.unwrap();
    assert_eq!(client.flush().await.unwrap(), 1);

    assert_eq!(kinds(&mock.events.lock().unwrap()), changes(&[1, 2, 3]));
    assert!(std::fs::read_to_string(&outbox).unwrap().trim().is_empty());
}

#[tokio::test]
async fn failure_while_resending_keeps_undelivered_events_in_order() {
    let mock = Arc::new(Mock::default());
    let url = serve(mock.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let outbox = dir.path().join("outbox.jsonl");
    let client = SyncClient::new(config(&url, Some(TOKEN), &outbox));

    client.queue_events(vec![event(1), event(400), event(2), event(500), event(3)]).await.unwrap();
    assert!(matches!(client.flush().await, Err(SyncError::Server { status: 500, .. })));
    assert_eq!(kinds(&mock.events.lock().unwrap()), changes(&[1, 2]));
    let left: Vec<TimedEvent> = std::fs::read_to_string(&outbox).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(kinds(&left), changes(&[500, 3]));

    mock.server_healed.store(true, Ordering::SeqCst);
    assert_eq!(client.flush().await.unwrap(), 0);
    assert_eq!(kinds(&mock.events.lock().unwrap()), changes(&[1, 2, 500, 3]));
    assert!(std::fs::read_to_string(&outbox).unwrap().trim().is_empty());
}

#[tokio::test]
async fn commands_are_handled_acked_and_survive_reconnect() {
    let mock = Arc::new(Mock::default());
    mock.pending.lock().unwrap().push(Command { id: 1, kind: CommandKind::OrganizeRequest });
    let url = serve(mock.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(SyncClient::new(config(&url, Some(TOKEN), &dir.path().join("outbox.jsonl"))));
    let mut status = client.status();

    let handled = Arc::new(Mutex::new(Vec::new()));
    let cancel = CancellationToken::new();
    let task = tokio::spawn({
        let (client, cancel, handled) = (client.clone(), cancel.clone(), handled.clone());
        async move {
            client
                .run(cancel, move |cmd| {
                    let handled = handled.clone();
                    async move {
                        handled.lock().unwrap().push(cmd.id);
                        if cmd.id == 101 { Err("font missing".to_string()) } else { Ok(()) }
                    }
                })
                .await
        }
    });

    status.wait_for(|s| *s == Status::Connected).await.unwrap();
    // pending (1) and pushed (101) on the first connection, which the mock then drops; 102 after reconnect.
    wait_until("three acks", || mock.acks.lock().unwrap().len() == 3).await;
    assert_eq!(*mock.acks.lock().unwrap(), [(1, true), (101, false), (102, true)]);
    assert_eq!(*handled.lock().unwrap(), [1, 101, 102]);
    assert_eq!(mock.ws_connects.load(Ordering::SeqCst), 2);

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(5), task).await.expect("run stops on cancel").unwrap();
    assert_eq!(*status.borrow(), Status::Offline);
}

#[tokio::test]
async fn non_command_messages_arrive_on_notices() {
    let url = serve(Arc::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(SyncClient::new(config(&url, Some(TOKEN), &dir.path().join("outbox.jsonl"))));
    let mut notices = client.notices().expect("the first call subscribes");
    assert!(client.notices().is_none(), "only one subscriber");

    let cancel = CancellationToken::new();
    let task = tokio::spawn({
        let (client, cancel) = (client.clone(), cancel.clone());
        async move { client.run(cancel, |_| async { Ok(()) }).await }
    });
    let msg = tokio::time::timeout(Duration::from_secs(5), notices.recv()).await.expect("no notice").unwrap();
    assert!(matches!(msg, ServerMessage::Identity { ref name, next_name_change: None } if name == "Budi 1"), "{msg:?}");

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(5), task).await.expect("run stops on cancel").unwrap();
}

#[tokio::test]
async fn update_release_and_download() {
    const FILE: &[u8] = b"MZ pretend setup program";
    let has_release = Arc::new(AtomicBool::new(false));
    let release = {
        let has = has_release.clone();
        move |h: HeaderMap| async move {
            auth(&h, true)?;
            if !has.load(Ordering::SeqCst) {
                return Err(StatusCode::NOT_FOUND);
            }
            Ok(Json(json!({ "manifest": "the manifest\n", "signature": "abcd\n" })))
        }
    };
    let file = |h: HeaderMap| async move { auth(&h, true).map(|()| FILE.to_vec()) };
    let app = Router::new().route("/api/v1/update", get(release)).route("/api/v1/update/file", get(file));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = tempfile::tempdir().unwrap();
    let client = SyncClient::new(config(&url, Some(TOKEN), &dir.path().join("outbox.jsonl")));
    assert_eq!(client.update_release().await.unwrap(), None);
    has_release.store(true, Ordering::SeqCst);
    assert_eq!(client.update_release().await.unwrap(), Some(("the manifest\n".to_string(), "abcd\n".to_string())));

    let to = dir.path().join("setup.part");
    client.download_update(&to, FILE.len() as u64).await.unwrap();
    assert_eq!(std::fs::read(&to).unwrap(), FILE);
    // More bytes than the manifest allows: stopped, and nothing is left behind.
    assert!(client.download_update(&to, FILE.len() as u64 - 1).await.is_err());
    assert!(!to.exists());

    let anonymous = SyncClient::new(config(&url, None, dir.path()));
    assert!(matches!(anonymous.update_release().await, Err(SyncError::Server { status: 401, .. })));
}

/// A manifest response past 64 KiB is not read to the end: no release.
#[tokio::test]
async fn an_oversized_update_release_is_none() {
    let big = || async { Json(json!({ "manifest": "x".repeat(70 * 1024), "signature": "abcd\n" })) };
    let app = Router::new().route("/api/v1/update", get(big));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let client = SyncClient::new(config(&url, Some(TOKEN), &dir.path().join("outbox.jsonl")));
    assert_eq!(client.update_release().await.unwrap(), None);
}

/// An old server without the update routes answers with the admin UI's HTML (200): that is "no release", not an error.
#[tokio::test]
async fn update_release_from_an_old_server_is_none() {
    let html = || async { ([("content-type", "text/html")], "<!doctype html><title>Typeshelf</title>") };
    let app = Router::new().route("/api/v1/update", get(html));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let client = SyncClient::new(config(&url, Some(TOKEN), &dir.path().join("outbox.jsonl")));
    assert_eq!(client.update_release().await.unwrap(), None);
}

fn set(uid: &str, name: &str) -> SyncSet {
    SyncSet { uid: uid.into(), name: name.into(), note: String::new(), updated_at: "2026-01-01T00:00:00.000000Z".into(), deleted_at: None, items: vec![] }
}

fn sets(names: &[(&str, &str)]) -> OrganizerDoc {
    OrganizerDoc { collections: vec![], sets: names.iter().map(|(u, n)| set(u, n)).collect() }
}

#[tokio::test]
async fn push_retries_after_conflict() {
    let mock = Arc::new(Mock::default());
    *mock.organizer.lock().unwrap() = Some((3, sets(&[("a", "Remote")])));
    // Another computer pushes "Other" between this computer's GET and PUT.
    *mock.organizer_sneak.lock().unwrap() = Some(sets(&[("b", "Other")]));
    let url = serve(mock.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let client = SyncClient::new(config(&url, Some(TOKEN), &dir.path().join("outbox.jsonl")));

    let local = sets(&[("c", "Local")]);
    let got = client.sync_organizer(&local, false).await.unwrap().expect("the server syncs organizers");
    let all = sets(&[("a", "Remote"), ("b", "Other"), ("c", "Local")]);
    assert_eq!(got, Organizer { rev: 5, doc: all.clone() }, "nothing is lost");
    let puts = mock.organizer_puts.lock().unwrap().clone();
    assert_eq!(puts.iter().map(|p| p.0).collect::<Vec<_>>(), [3, 4], "one retry, on the rev the conflict returned");
    assert_eq!(puts[1].1, all, "the retry carries the merge with the server's current document");
}

#[tokio::test]
async fn organizer_pushes_only_what_the_server_lacks() {
    let mock = Arc::new(Mock::default());
    let url = serve(mock.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let client = SyncClient::new(config(&url, Some(TOKEN), &dir.path().join("outbox.jsonl")));
    assert_eq!(client.sync_organizer(&sets(&[("c", "Local")]), true).await.unwrap(), None, "404: an older server");

    let remote = sets(&[("a", "Remote"), ("c", "Local")]);
    *mock.organizer.lock().unwrap() = Some((7, remote.clone()));
    let got = client.sync_organizer(&sets(&[("c", "Local")]), false).await.unwrap().unwrap();
    assert_eq!(got, Organizer { rev: 7, doc: remote.clone() }, "the pull is adopted");
    assert!(mock.organizer_puts.lock().unwrap().is_empty(), "the server already has everything: no push");

    client.sync_organizer(&sets(&[("c", "Local")]), true).await.unwrap().unwrap();
    assert_eq!(mock.organizer_puts.lock().unwrap().len(), 1, "dirty pushes anyway");
}
