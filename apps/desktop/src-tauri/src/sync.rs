//! The server connection: one `SyncClient` at a time, replaced whenever the connection settings change.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::UnboundedReceiver;
use typeshelf_engine::{ConnectionStatus, Engine, EngineError};
use typeshelf_protocol::{Command, CommandKind, RegisterRequest, TimedEvent};
use typeshelf_sync::{CancellationToken, Config, Status, SyncClient, SyncError};

use crate::{blocking, Ctx};

/// Installer defaults (Settings › Connection › Reset), baked in at compile time.
pub const DEFAULT_SERVER_URL: &str = match option_env!("TYPESHELF_SERVER_URL") {
    Some(v) => v,
    None => "http://localhost:8080",
};
pub const DEFAULT_INSTALL_KEY: &str = match option_env!("TYPESHELF_INSTALL_KEY") {
    Some(v) => v,
    None => "",
};

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
    let handler = {
        let (app, client) = (app.clone(), client.clone());
        move |cmd: Command| {
            let (app, client) = (app.clone(), client.clone());
            async move { handle(&app, &client, cmd.kind).await }
        }
    };
    client.run(cancel, handler).await;
}

/// True once there is a token. An install that has a name but no token (the connection was changed)
/// registers again with that name; one without a name waits for onboarding.
async fn ensure_registered(app: &AppHandle, engine: &Arc<Engine>, client: &SyncClient, cancel: &CancellationToken) -> bool {
    loop {
        let Ok((token, name)) = blocking(engine, |e| Ok((e.token()?, e.get_app_state()?.name))).await else {
            return false;
        };
        if token.is_some() {
            return true;
        }
        if name.is_empty() {
            // Not onboarded yet (onboarding restarts sync): report whether the server is reachable,
            // so the onboarding screen shows the real status instead of "Not connected".
            let (status, error) = match client.health().await {
                Ok(()) => (ConnectionStatus::Connected, None),
                Err(SyncError::Offline) => (ConnectionStatus::Offline, None),
                Err(e) => (ConnectionStatus::Error, Some(e.to_string())),
            };
            engine.set_connection_status(status, error);
            emit_connection(app).await;
        } else {
            match register(engine, client, &name).await {
                Ok(()) => return true,
                Err(e) => {
                    tracing::warn!("registering again: {e}");
                    let status = if e.code == "offline" { ConnectionStatus::Offline } else { ConnectionStatus::Error };
                    engine.set_connection_status(status, (status == ConnectionStatus::Error).then(|| e.message.clone()));
                    emit_connection(app).await;
                }
            }
        }
        // ponytail: fixed retry; the WebSocket loop has the real backoff once registered.
        let wait = if name.is_empty() { 15 } else { 30 };
        tokio::select! {
            _ = cancel.cancelled() => return false,
            _ = tokio::time::sleep(Duration::from_secs(wait)) => {}
        }
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
            Status::Error(e) => (ConnectionStatus::Error, Some(e)),
        };
        engine.set_connection_status(s, error);
        if connected {
            on_connect(&app, &engine, &client).await;
        }
        emit_connection(&app).await;
    }
}

/// Uploads the active-font snapshot and picks up name/policy changes made on the server.
async fn on_connect(app: &AppHandle, engine: &Arc<Engine>, client: &SyncClient) {
    push_snapshot(engine, client).await;
    match client.me().await {
        Ok(me) => {
            let changed = blocking(engine, move |e| {
                e.set_policy(&me.policy)?;
                let state = e.get_app_state()?;
                let next = me.next_name_change.map(|d| d.to_rfc3339());
                if state.name == me.name && state.next_name_change == next {
                    return Ok(None);
                }
                e.set_identity(&me.name, me.next_name_change)?;
                Ok(Some(json!({ "name": me.name, "nextNameChange": next })))
            })
            .await;
            match changed {
                Ok(Some(payload)) => {
                    let _ = app.emit("name-changed", payload);
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("storing /me: {e}"),
            }
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

/// Moves engine events into the persistent outbox of whichever client is current.
pub async fn drain_events(app: AppHandle, mut rx: UnboundedReceiver<TimedEvent>) {
    while let Some(first) = rx.recv().await {
        let mut batch = vec![first];
        while let Ok(ev) = rx.try_recv() {
            batch.push(ev);
        }
        let client = app.state::<Ctx>().client();
        if let Err(e) = client.queue_events(batch).await {
            tracing::warn!("queueing events: {e}");
        }
    }
}
