//! Desktop API, `/api/v1`.

use crate::auth::{self, Install, InstallKey};
use crate::{db, hub, library, ApiError, ApiResult, AppState};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use typeshelf_core::name_rule::validate_name;
use typeshelf_protocol::*;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/installs", post(register))
        .route("/me", get(me))
        .route("/me/name", put(change_name))
        .route("/me/events", post(events))
        .route("/me/fonts", put(snapshot))
        .route("/me/commands", get(commands))
        .route("/me/commands/{id}/ack", post(ack))
        .route("/me/ws", get(ws))
        .route("/me/library", get(library::my_library))
        .route("/library/files/{hash}", get(library::download))
}

async fn health(_: InstallKey) -> StatusCode {
    StatusCode::NO_CONTENT
}

fn invalid_name(msg: &str) -> ApiError {
    ApiError::bad_request("invalid_name", msg)
}

async fn register(State(s): State<AppState>, _: InstallKey, Json(req): Json<RegisterRequest>) -> ApiResult<Json<RegisterResponse>> {
    let name = validate_name(&req.name).map_err(invalid_name)?;
    let install_id = req.install_id.trim();
    if install_id.is_empty() || install_id.len() > 100 {
        return Err(ApiError::bad_request("invalid_install_id", "install_id must be 1–100 characters"));
    }
    let token = auth::random_hex(32);
    let now = db::now();
    let mut db = s.db();
    let tx = db.transaction()?;
    // `last_seen` moves on the first authenticated request (microsecond timestamps), so an equal pair means the
    // desktop never used its token, most likely because it never got this response: hand out a new one.
    let unused: Option<bool> =
        tx.query_row("SELECT last_seen = first_seen FROM installs WHERE id = ?1", [install_id], |r| r.get(0)).optional()?;
    match unused {
        Some(true) => {
            tx.execute("UPDATE installs SET token_hash = ?1 WHERE id = ?2", params![auth::token_hash(&token), install_id])?;
            let settings = db::settings(&tx)?;
            let name = db::install_name(&tx, install_id)?.unwrap_or_default();
            let next_name_change = db::next_name_change(&tx, install_id, settings.name_change_days)?;
            tx.commit()?;
            return Ok(Json(RegisterResponse { token, name, next_name_change, policy: settings.policy() }));
        }
        Some(false) => return Err(ApiError::new(StatusCode::CONFLICT, "already_registered", "this install is already registered")),
        None => {}
    }
    tx.execute(
        "INSERT INTO installs (id, token_hash, name, os, app_version, first_seen, last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![install_id, auth::token_hash(&token), name, req.os, req.app_version, now],
    )?;
    tx.execute(
        "INSERT INTO name_changes (install_id, old_name, new_name, by, at) VALUES (?1, NULL, ?2, 'first', ?3)",
        params![install_id, name, now],
    )?;
    db::insert_event(&tx, install_id, "name", "user", &format!("Joined as {name}"), "{}", &now)?;
    let settings = db::settings(&tx)?;
    library::welcome_commands(&tx, &s.hub, install_id, settings.deliver_library_to_new)?;
    tx.commit()?;
    Ok(Json(RegisterResponse { token, name, next_name_change: None, policy: settings.policy() }))
}

async fn me(State(s): State<AppState>, install: Install) -> ApiResult<Json<Value>> {
    let db = s.db();
    let settings = db::settings(&db)?;
    let name = db::install_name(&db, &install.id)?.unwrap_or_default();
    let next = db::next_name_change(&db, &install.id, settings.name_change_days)?;
    Ok(Json(json!({ "name": name, "next_name_change": next, "policy": settings.policy() })))
}

async fn change_name(State(s): State<AppState>, install: Install, Json(req): Json<NameChange>) -> ApiResult<Json<NameStatus>> {
    let name = validate_name(&req.name).map_err(invalid_name)?;
    let db = s.db();
    let days = db::settings(&db)?.name_change_days;
    let current = db::install_name(&db, &install.id)?.unwrap_or_default();
    if name == current {
        let next_name_change = db::next_name_change(&db, &install.id, days)?;
        return Ok(Json(NameStatus { name, next_name_change }));
    }
    if let Some(next) = db::next_name_change(&db, &install.id, days)? {
        return Err(ApiError::new(StatusCode::CONFLICT, "name_locked", format!("the name can be changed again on {}", next.date_naive()))
            .with("name", &current)
            .with("next_name_change", next));
    }
    let now = db::now();
    db.execute("UPDATE installs SET name = ?1 WHERE id = ?2", params![name, install.id])?;
    db.execute(
        "INSERT INTO name_changes (install_id, old_name, new_name, by, at) VALUES (?1, ?2, ?3, 'user', ?4)",
        params![install.id, current, name, now],
    )?;
    db::insert_event(&db, &install.id, "name", "user", &format!("Changed name from {current} to {name}"), "{}", &now)?;
    let next_name_change = db::next_name_change(&db, &install.id, days)?;
    Ok(Json(NameStatus { name, next_name_change }))
}

async fn events(State(s): State<AppState>, install: Install, Json(batch): Json<EventBatch>) -> ApiResult<StatusCode> {
    let mut db = s.db();
    let tx = db.transaction()?;
    for e in &batch.events {
        record_event(&tx, &install.id, &db::ts(e.at), &e.event)?;
    }
    if let Some(days) = db::settings(&tx)?.timeline_retention_days {
        let cutoff = db::ts(chrono::Utc::now() - chrono::Duration::days(days.into()));
        tx.execute("DELETE FROM events WHERE install_id = ?1 AND at < ?2", params![install.id, cutoff])?;
    }
    tx.commit()?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn source_str(s: Source) -> &'static str {
    match s {
        Source::User => "user",
        Source::Admin => "admin",
        Source::ProjectSet => "project_set",
        Source::Library => "library",
    }
}

fn label(f: &FontRef) -> String {
    format!("{} {}", f.family, f.style).trim().to_string()
}

/// `overridden` = (admin_at, user_at) when the user turned off an admin font.
fn upsert_font(c: &Connection, install_id: &str, f: &FontRef, active: bool, source: &str, at: &str, overridden: Option<(&str, &str)>) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO font_states (install_id, hash, ps_name, family, style, active, source, updated_at, admin_at, overridden_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(install_id, hash) DO UPDATE SET ps_name = excluded.ps_name, family = excluded.family, style = excluded.style,
           active = excluded.active, source = excluded.source, updated_at = excluded.updated_at,
           admin_at = excluded.admin_at, overridden_at = excluded.overridden_at",
        params![install_id, f.hash, f.ps_name, f.family, f.style, active, source, at, overridden.map(|o| o.0), overridden.map(|o| o.1)],
    )?;
    Ok(())
}

/// Adds one desktop event to the timeline and keeps `font_states` current.
fn record_event(c: &Connection, install_id: &str, at: &str, event: &Event) -> rusqlite::Result<()> {
    let by_of = |s: &Source| if *s == Source::Admin { "admin" } else { "user" };
    let (kind, by, text) = match event {
        Event::FontActivated { font, source } => {
            upsert_font(c, install_id, font, true, source_str(*source), at, None)?;
            ("font", by_of(source), format!("Turned on {}", label(font)))
        }
        Event::FontDeactivated { font, source, was_admin } => {
            let prev: Option<(String, bool, String)> = c
                .query_row(
                    "SELECT source, active, updated_at FROM font_states WHERE install_id = ?1 AND hash = ?2",
                    params![install_id, font.hash],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let admin_on = prev.as_ref().filter(|(src, active, _)| src == "admin" && *active).map(|p| p.2.as_str());
            if *source != Source::Admin && (*was_admin || admin_on.is_some()) {
                upsert_font(c, install_id, font, false, "admin", at, Some((admin_on.unwrap_or(at), at)))?;
                ("override", "user", format!("Turned off {}, which the admin turned on", label(font)))
            } else {
                upsert_font(c, install_id, font, false, source_str(*source), at, None)?;
                ("font", by_of(source), format!("Turned off {}", label(font)))
            }
        }
        Event::ActivationFailed { font, reason } => ("failure", "system", format!("Couldn't turn on {}: {reason}", label(font))),
        Event::ProjectSetToggled { name, on, fonts } => {
            ("font", "user", format!("Turned {} project set “{name}” ({fonts} fonts)", if *on { "on" } else { "off" }))
        }
        Event::OrganizeApplied { changes } => ("organize", "user", format!("Applied auto-organize ({changes} changes)")),
        Event::DuplicatesReport { duplicates, version_conflicts, name_clashes } => (
            "organize",
            "system",
            format!("Found {duplicates} duplicates, {version_conflicts} version conflicts, {name_clashes} name clashes"),
        ),
        Event::RepairRan { removed, reregistered } => {
            ("system", "system", format!("Repair removed {removed} stale entries and re-registered {reregistered} fonts"))
        }
    };
    let payload = serde_json::to_string(event).expect("event serializes");
    db::insert_event(c, install_id, kind, by, &text, &payload, at)
}

/// Replaces the active-font list; inactive rows are dropped except overrides, which stay visible to the admin.
async fn snapshot(State(s): State<AppState>, install: Install, Json(snap): Json<FontSnapshot>) -> ApiResult<StatusCode> {
    let now = db::now();
    let mut db = s.db();
    let tx = db.transaction()?;
    tx.execute("DELETE FROM font_states WHERE install_id = ?1 AND overridden_at IS NULL", [&install.id])?;
    for f in &snap.active {
        upsert_font(&tx, &install.id, &f.font, true, source_str(f.source), &now, None)?;
    }
    tx.commit()?;
    Ok(StatusCode::NO_CONTENT)
}

async fn commands(State(s): State<AppState>, install: Install) -> ApiResult<Json<Vec<Command>>> {
    Ok(Json(hub::pending(&s.db(), &install.id)?))
}

#[derive(Deserialize)]
struct AckBody {
    ok: bool,
    error: Option<String>,
}

async fn ack(State(s): State<AppState>, install: Install, Path(id): Path<i64>, Json(body): Json<AckBody>) -> ApiResult<StatusCode> {
    if !hub::ack(&s.db(), &install.id, id, body.ok, body.error.as_deref())? {
        return Err(ApiError::not_found("no such command"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn ws(State(s): State<AppState>, install: Install, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| run_socket(s, install.id, socket))
}

async fn send(socket: &mut WebSocket, cmd: Command) -> bool {
    let text = serde_json::to_string(&ServerMessage::Command(cmd)).expect("message serializes");
    socket.send(Message::Text(text.into())).await.is_ok()
}

async fn run_socket(s: AppState, install_id: String, mut socket: WebSocket) {
    // Register first, then replay the backlog; pushes with ids already replayed are skipped.
    let (conn, mut rx) = s.hub.connect(&install_id);
    let backlog = hub::pending(&s.db(), &install_id);
    let mut last_sent = 0;
    let mut open = true;
    match backlog {
        Ok(cmds) => {
            for cmd in cmds {
                last_sent = cmd.id;
                if !send(&mut socket, cmd).await {
                    open = false;
                    break;
                }
            }
        }
        Err(e) => {
            tracing::error!("loading commands for {install_id}: {e}");
            open = false;
        }
    }
    while open {
        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(cmd) if cmd.id > last_sent => {
                    let id = cmd.id;
                    open = send(&mut socket, cmd).await;
                    if open {
                        if let Err(e) = hub::mark_delivered(&s.db(), id) {
                            tracing::error!("marking command {id} delivered: {e}");
                        }
                    }
                }
                Some(_) => {}
                None => open = false, // replaced by a newer connection
            },
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(text))) => match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(ClientMessage::Ack { id, ok, error }) => {
                        if let Err(e) = hub::ack(&s.db(), &install_id, id, ok, error.as_deref()) {
                            tracing::error!("ack {id}: {e}");
                        }
                    }
                    Err(e) => tracing::debug!("ignoring client message: {e}"),
                },
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => open = false,
                Some(Ok(_)) => {}
            },
        }
    }
    s.hub.disconnect(&install_id, conn);
    let _ = s.db().execute("UPDATE installs SET last_seen = ?1 WHERE id = ?2", params![db::now(), install_id]);
}
