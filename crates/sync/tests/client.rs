//! Exercises SyncClient against a small in-process mock of the Desktop API.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
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
use typeshelf_protocol::*;
use typeshelf_sync::{CancellationToken, Config, Status, SyncClient, SyncError};

const KEY: &str = "install-key";
const TOKEN: &str = "install-token";

#[derive(Default)]
struct Mock {
    events: Mutex<Vec<TimedEvent>>,
    acks: Mutex<Vec<(i64, bool)>>,
    pending: Mutex<Vec<Command>>,
    ws_connects: AtomicUsize,
}

type S = State<Arc<Mock>>;

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
    Policy { name_change_days: 30, allow_connection_edit: true, notify_admin_changes: false }
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
