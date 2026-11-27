//! The server connection: one `SyncClient` at a time, replaced whenever the connection settings change.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Notify;
use typeshelf_engine::{ConnectionStatus, Engine, EngineError};
use typeshelf_protocol::{Command, CommandKind, RegisterRequest, ServerMessage, TimedEvent};
use typeshelf_sync::{CancellationToken, Config, Status, SyncClient, SyncError};

use crate::{blocking, Ctx};

/// Installer defaults (Settings › Connection › Reset), baked in at compile time. A build without a server
/// baked in is the standalone build: no server until one is added in Settings › Connection.
pub const DEFAULT_SERVER_URL: &str = match option_env!("TYPESHELF_SERVER_URL") {
    Some(v) => v,
    None => "",
};
pub const DEFAULT_INSTALL_KEY: &str = match option_env!("TYPESHELF_INSTALL_KEY") {
    Some(v) => v,
    None => "",
};

/// Marker next to the database: the person saved an empty server address, so the baked-in one stays out.
// ponytail: a file because the engine has no key-value call for the shell; move it into the settings store
// if the engine gains one.
const SERVER_CLEARED: &str = "server-cleared";

/// Whether start-up fills in the installer defaults: nothing stored yet, and the address was not cleared.
pub fn seed_defaults(data_dir: &Path, stored_url: &str) -> bool {
    stored_url.is_empty() && !data_dir.join(SERVER_CLEARED).exists()
}

/// Set when a connection save leaves no address, lifted by any save with one (or "Use installer defaults").
pub fn set_server_cleared(data_dir: &Path, on: bool) {
    let path = data_dir.join(SERVER_CLEARED);
    let res = if on { std::fs::write(&path, b"") } else { std::fs::remove_file(&path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }) };
    if let Err(e) = res {
        tracing::warn!("remembering that the server address was cleared: {e}");
    }
}

pub fn new_client(data_dir: &Path, server_url: String, install_key: String, token: Option<String>) -> Arc<SyncClient> {
    Arc::new(SyncClient::new(Config { server_url, install_key, token, outbox_path: data_dir.join("outbox.jsonl") }))
}

pub fn sync_err(e: SyncError) -> EngineError {
    match e {
        SyncError::Offline => EngineError::new("offline", "The server can't be reached."),
        SyncError::NameLocked { next_name_change } => {
            EngineError::new("name_locked", format!("The name can be changed again on {}.", next_name_change.date_naive()))
                .with("nextNameChange", next_name_change.to_rfc3339())
        }
        SyncError::Server { code, message, status } => {
            let code = if code.is_empty() { format!("http_{status}") } else { code };
            EngineError::new(&code, message)
        }
        e => EngineError::new("server", e.to_string()),
    }
}

pub async fn emit_connection(app: &AppHandle) {
    let engine = app.state::<Ctx>().engine.clone();
    if let Ok(c) = blocking(&engine, |e| e.get_connection()).await {
        let _ = app.emit("connection-changed", c);
    }
}

/// `POST /installs` with the stored install id; stores token, identity and policy.
pub async fn register(engine: &Arc<Engine>, client: &SyncClient, name: &str) -> Result<(), EngineError> {
    let install_id = blocking(engine, |e| e.install_id()).await?;
    let req = RegisterRequest {
        install_id,
        name: name.into(),
        os: std::env::consts::OS.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
    };
    let res = client.register(req).await.map_err(sync_err)?;
    tracing::info!(name = %res.name, "registered with the server");
    blocking(engine, move |e| {
        e.set_token(Some(&res.token))?;
        e.set_identity(&res.name, res.next_name_change)?;
        e.set_policy(&res.policy)
    })
    .await
}

/// Stops the current client and starts a new one from the stored connection settings.
pub async fn restart(app: &AppHandle) -> Result<(), EngineError> {
    let ctx = app.state::<Ctx>();
    let (conn, token) = ctx.run(|e| Ok((e.get_connection()?, e.token()?))).await?;
    let client = new_client(&ctx.data_dir, conn.server_url, conn.install_key, token);
    let cancel = CancellationToken::new();
    let old = std::mem::replace(&mut *ctx.sync.lock().unwrap(), (client.clone(), cancel.clone()));
    old.1.cancel();
    ctx.engine.set_connection_status(ConnectionStatus::Offline, None);
    emit_connection(app).await;
    tauri::async_runtime::spawn(run(app.clone(), client, cancel));
    Ok(())
}

async fn run(app: AppHandle, client: Arc<SyncClient>, cancel: CancellationToken) {
    let engine = app.state::<Ctx>().engine.clone();
    if !ensure_registered(&app, &engine, &client, &cancel).await {
        return;
    }
    tauri::async_runtime::spawn(watch_status(app.clone(), client.clone(), cancel.clone()));
    tauri::async_runtime::spawn(watch_dropped_events(app.clone(), client.clone(), cancel.clone()));
    let organizer_changed = Arc::new(Notify::new());
    tauri::async_runtime::spawn(watch_organizer(app.clone(), client.clone(), organizer_changed.clone(), cancel.clone()));
    if let Some(notices) = client.notices() {
        tauri::async_runtime::spawn(watch_notices(app.clone(), notices, organizer_changed, cancel.clone()));
    }
    let handler = {
        let (app, client) = (app.clone(), client.clone());
        move |cmd: Command| {
            let (app, client) = (app.clone(), client.clone());
            async move { handle(&app, &client, cmd.kind).await }
        }
    };
    client.run(cancel, handler).await;
}

/// What the sync loop does next, from the stored connection and identity.
#[derive(Debug, PartialEq)]
enum Step {
    /// No server address: nothing to connect to until one is added in Settings (which restarts sync).
    Standalone,
    /// Not onboarded yet: only report whether the server answers.
    Probe,
    /// A name but no token (onboarded offline, or the connection was changed): register with that name.
    Register(String),
    Connect,
}

fn next_step(server_configured: bool, token: bool, name: &str) -> Step {
    match (server_configured, token, name.is_empty()) {
        (false, _, _) => Step::Standalone,
        (true, true, _) => Step::Connect,
        (true, false, true) => Step::Probe,
        (true, false, false) => Step::Register(name.into()),
    }
}

/// True once there is a token; false when there is no server or the loop was cancelled.
async fn ensure_registered(app: &AppHandle, engine: &Arc<Engine>, client: &SyncClient, cancel: &CancellationToken) -> bool {
    loop {
        let Ok((configured, token, name)) = blocking(engine, |e| {
            let s = e.get_app_state()?;
            Ok((s.server_configured, e.token()?.is_some(), s.name))
        })
        .await
        else {
            return false;
        };
        let wait = match next_step(configured, token, &name) {
            Step::Standalone => return false,
            Step::Connect => return true,
            Step::Probe => {
                // Onboarding restarts sync: report whether the server is reachable, so the onboarding
                // screen shows the real status (and offers "Continue offline" when it isn't).
                let (status, error) = match client.health().await {
                    Ok(()) => (ConnectionStatus::Connected, None),
                    Err(SyncError::Offline) => (ConnectionStatus::Offline, None),
                    Err(e) => (ConnectionStatus::Error, Some(e.to_string())),
                };
                engine.set_connection_status(status, error);
                emit_connection(app).await;
                15
            }
            Step::Register(name) => match register(engine, client, &name).await {
                Ok(()) => {
                    // The server's name is stored, which can differ from one changed while this request ran.
                    if let Ok(s) = blocking(engine, |e| e.get_app_state()).await {
                        let _ = app.emit("name-changed", json!({ "name": s.name, "nextNameChange": s.next_name_change }));
                    }
                    return true;
                }
                Err(e) if e.code == "invalid_name" => {
                    // The server refused a name chosen offline: retrying can't help. Forget it so the app goes
                    // back to onboarding, which shows why and asks for another one.
                    tracing::warn!("the server refused the name: {e}");
                    let _ = blocking(engine, |e| e.set_identity("", None)).await;
                    let why = format!("Your server didn’t accept the name “{name}”: {} Choose another name.", e.message);
                    engine.set_connection_status(ConnectionStatus::Error, Some(why));
                    emit_connection(app).await;
                    let _ = app.emit("name-changed", json!({ "name": "", "nextNameChange": null }));
                    return false;
                }
                Err(e) => {
                    tracing::warn!("registering: {e}");
                    let status = if e.code == "offline" { ConnectionStatus::Offline } else { ConnectionStatus::Error };
                    engine.set_connection_status(status, (status == ConnectionStatus::Error).then(|| e.message.clone()));
                    emit_connection(app).await;
                    30
                }
            },
        };
        // ponytail: fixed retry; the WebSocket loop has the real backoff once registered.
        tokio::select! {
            _ = cancel.cancelled() => return false,
            _ = tokio::time::sleep(Duration::from_secs(wait)) => {}
        }
    }
}

/// The client flushes the outbox itself while connected; events the server rejects are dropped there and reported here
/// (log, plus `events-dropped` so the UI can say the server may need an update, e.g. a font request an old server refused).
async fn watch_dropped_events(app: AppHandle, client: Arc<SyncClient>, cancel: CancellationToken) {
    let mut rx = client.dropped_events();
    let mut seen = *rx.borrow_and_update();
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            r = rx.changed() => if r.is_err() { return },
        }
        let total = *rx.borrow_and_update();
        if total > seen {
            tracing::warn!(count = total - seen, "the server rejected these events, so they were dropped and will not be retried");
            let _ = app.emit("events-dropped", json!({ "count": total - seen }));
        }
        seen = total;
    }
}

async fn watch_status(app: AppHandle, client: Arc<SyncClient>, cancel: CancellationToken) {
    let engine = app.state::<Ctx>().engine.clone();
    let mut rx = client.status();
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            r = rx.changed() => if r.is_err() { return },
        }
        let status = rx.borrow_and_update().clone();
        tracing::info!(?status, "server connection");
        let connected = status == Status::Connected;
        let (s, error) = match status {
            Status::Connected => (ConnectionStatus::Connected, None),
            Status::Offline => (ConnectionStatus::Offline, None),
            Status::Error(e) => (ConnectionStatus::Error, Some(refused(&client, e).await)),
        };
        engine.set_connection_status(s, error);
        if connected {
            on_connect(&app, &engine, &client).await;
        }
        emit_connection(&app).await;
    }
}

/// Shown when the admin removed this computer (`401 revoked`); saving the connection again registers anew.
pub const REVOKED: &str = "This computer was removed by your admin. Save the connection again to rejoin.";

/// The text for a refused connection. The WebSocket handshake error carries only the HTTP status, so a 401 is
/// looked up with one `GET /me` (read-only, the request every connect makes anyway) to tell a removed computer.
// ponytail: costs one extra request per failed reconnect while refused; read the code from the handshake body if
// the sync crate starts exposing it.
async fn refused(client: &SyncClient, error: String) -> String {
    if !error.contains("401") {
        return error;
    }
    match client.me().await {
        Err(SyncError::Server { code, .. }) if code == "revoked" => REVOKED.into(),
        _ => error,
    }
}

/// Live messages from the server other than commands. `identity`: the person's name or limit changed on another of
/// their computers or by an admin, stored like `GET /me` on connect. `organizer_changed`: wakes `watch_organizer`.
async fn watch_notices(app: AppHandle, mut rx: UnboundedReceiver<ServerMessage>, organizer_changed: Arc<Notify>, cancel: CancellationToken) {
    let engine = app.state::<Ctx>().engine.clone();
    loop {
        let msg = tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            m = rx.recv() => match m { Some(m) => m, None => return },
        };
        match msg {
            ServerMessage::Identity { name, next_name_change } => store_identity(&app, &engine, name, next_name_change).await,
            ServerMessage::OrganizerChanged { .. } => organizer_changed.notify_one(),
            _ => {}
        }
    }
}

/// Collections and project sets follow the person (`GET`/`PUT /me/organizer`): one round on every connect, on
/// `organizer_changed`, and within about 2 s of a local change. Rounds run one at a time. A server without
/// organizer sync (404) turns it off until the next connect; after a failed round, local changes wait a minute
/// (or the next connect or notice) before the next try, so a refusing server is not asked every 2 s.
async fn watch_organizer(app: AppHandle, client: Arc<SyncClient>, changed: Arc<Notify>, cancel: CancellationToken) {
    let engine = app.state::<Ctx>().engine.clone();
    let mut status = client.status();
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let (mut off, mut wait_until) = (false, tokio::time::Instant::now());
    loop {
        let asked = tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            r = status.changed() => {
                if r.is_err() {
                    return;
                }
                off = false;
                true
            }
            _ = changed.notified() => true,
            _ = tick.tick() => false,
        };
        if off || *status.borrow_and_update() != Status::Connected {
            continue;
        }
        if !asked && (tokio::time::Instant::now() < wait_until || !blocking(&engine, |e| e.organizer_sync_state()).await.is_ok_and(|s| s.1)) {
            continue;
        }
        match organizer_round(&engine, &client, &cancel).await {
            Round::Done => {}
            Round::Failed => wait_until = tokio::time::Instant::now() + Duration::from_secs(60),
            Round::Unsupported => off = true,
        }
    }
}

enum Round {
    Done,
    Failed,
    Unsupported,
}

/// Pull, merge, push if the server lacks something, then adopt the server's answer locally (which emits
/// `library-changed` when anything changed, so the lists reload). Once `cancel` fires (the connection was changed
/// or replaced) nothing more is written: the old server's document must not reach this computer.
async fn organizer_round(engine: &Arc<Engine>, client: &SyncClient, cancel: &CancellationToken) -> Round {
    let (local, dirty) = match blocking(engine, |e| Ok((e.organizer_doc()?, e.organizer_sync_state()?.1))).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("reading collections and project sets for sync: {e}");
            return Round::Failed;
        }
    };
    let answer = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Round::Done,
        r = client.sync_organizer(&local, dirty) => r,
    };
    let cancel = cancel.clone();
    match answer {
        Ok(Some(o)) => match blocking(engine, move |e| {
            // Checked again under the engine call, right before the writes.
            if cancel.is_cancelled() {
                return Ok(());
            }
            e.apply_organizer(&o.doc)?;
            e.set_organizer_synced(o.rev)
        })
        .await
        {
            Ok(()) => Round::Done,
            Err(e) => {
                tracing::warn!("applying synced collections and project sets: {e}");
                Round::Failed
            }
        },
        Ok(None) | Err(SyncError::Server { status: 404, .. }) => {
            tracing::info!("the server doesn't sync collections and project sets; off until the next connect");
            Round::Unsupported
        }
        Err(e) => {
            tracing::warn!("syncing collections and project sets: {e}");
            Round::Failed
        }
    }
}

/// Stores the server's name and next allowed change; emits `name-changed` when either differs from what is stored.
async fn store_identity(app: &AppHandle, engine: &Arc<Engine>, name: String, next_name_change: Option<DateTime<Utc>>) {
    let changed = blocking(engine, move |e| {
        let state = e.get_app_state()?;
        let next = next_name_change.map(|d| d.to_rfc3339());
        if state.name == name && state.next_name_change == next {
            return Ok(None);
        }
        e.set_identity(&name, next_name_change)?;
        Ok(Some(json!({ "name": name, "nextNameChange": next })))
    })
    .await;
    match changed {
        Ok(Some(payload)) => {
            let _ = app.emit("name-changed", payload);
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("storing the name from the server: {e}"),
    }
}

/// Uploads the active-font snapshot and picks up name/policy changes made on the server.
async fn on_connect(app: &AppHandle, engine: &Arc<Engine>, client: &SyncClient) {
    push_snapshot(engine, client).await;
    match client.me().await {
        Ok(me) => {
            let policy = me.policy;
            if let Err(e) = blocking(engine, move |e| e.set_policy(&policy)).await {
                tracing::warn!("storing the policy: {e}");
            }
            store_identity(app, engine, me.name, me.next_name_change).await;
        }
        Err(e) => tracing::warn!("GET /me: {e}"),
    }
    let _ = blocking(engine, |e| e.set_last_sync(Utc::now())).await;
}

pub async fn push_snapshot(engine: &Arc<Engine>, client: &SyncClient) {
    match blocking(engine, |e| e.snapshot()).await {
        Ok(snapshot) => match client.put_snapshot(&snapshot).await {
            Ok(()) => tracing::info!(active = snapshot.active.len(), "snapshot uploaded"),
            Err(e) => tracing::warn!("uploading snapshot: {e}"),
        },
        Err(e) => tracing::warn!("snapshot: {e}"),
    }
}

/// Applies an admin command; the result is acked by the client.
async fn handle(app: &AppHandle, client: &SyncClient, kind: CommandKind) -> Result<(), String> {
    tracing::info!(?kind, "admin command");
    let engine = app.state::<Ctx>().engine.clone();
    let k = kind.clone();
    let mut res = blocking(&engine, move |e| e.apply_command(&k)).await;
    let download = matches!(&res, Err(e) if e.code == "download_required");
    if let (true, CommandKind::Deliver { file, activate }) = (download, &kind) {
        res = match client.download_library_file(&file.hash).await {
            Ok(bytes) => {
                let (file, activate) = (file.clone(), *activate);
                blocking(&engine, move |e| e.deliver(&file, &bytes, activate)).await
            }
            Err(e) => Err(sync_err(e)),
        };
    }
    if matches!(kind, CommandKind::PolicyChanged { .. }) {
        emit_connection(app).await;
    }
    res.map_err(|e| {
        tracing::warn!("admin command failed: {e}");
        e.to_string()
    })
}

/// Moves engine events into the persistent outbox of whichever client is current. Without a server they
/// are dropped: nobody will ever read them, so they must not pile up.
pub async fn drain_events(app: AppHandle, mut rx: UnboundedReceiver<TimedEvent>) {
    while let Some(first) = rx.recv().await {
        let mut batch = vec![first];
        while let Ok(ev) = rx.try_recv() {
            batch.push(ev);
        }
        let engine = app.state::<Ctx>().engine.clone();
        if !blocking(&engine, |e| e.get_app_state()).await.is_ok_and(|s| s.server_configured) {
            continue;
        }
        let client = app.state::<Ctx>().client();
        if let Err(e) = client.queue_events(batch).await {
            tracing::warn!("queueing events: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_server_configured_means_no_sync_attempts() {
        // Whatever the identity, no server address means the loop stops before any request.
        assert_eq!(next_step(false, false, ""), Step::Standalone);
        assert_eq!(next_step(false, false, "Ana"), Step::Standalone);
        assert_eq!(next_step(false, true, "Ana"), Step::Standalone);
    }

    #[test]
    fn a_cleared_server_address_is_not_filled_in_again_at_start() {
        let dir = std::env::temp_dir().join(format!("typeshelf-seed-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(seed_defaults(&dir, ""), "first start: the installer defaults are filled in");
        assert!(!seed_defaults(&dir, "https://fonts.example.com"), "an address is already stored");
        set_server_cleared(&dir, true);
        assert!(!seed_defaults(&dir, ""), "the person cleared the address");
        set_server_cleared(&dir, false);
        assert!(seed_defaults(&dir, ""), "a later save or \"Use installer defaults\" lifts it");
        set_server_cleared(&dir, false);
    }

    #[test]
    fn registers_later_with_the_saved_name() {
        // Onboarded offline: a name, no token yet. The loop registers with that name.
        assert_eq!(next_step(true, false, "Ana"), Step::Register("Ana".into()));
        assert_eq!(next_step(true, false, ""), Step::Probe, "not onboarded: only report whether the server answers");
        assert_eq!(next_step(true, true, "Ana"), Step::Connect);
    }
}
