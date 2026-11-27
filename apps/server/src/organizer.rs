//! The person's synced collections and project sets: `GET/PUT /me/organizer`.
//!
//! One `organizer_docs` row per install that pushed. A read merges every row of the person's group (revoked
//! members included), so merge, split and remove-a-computer need no hooks; a push stores the merge under the
//! person and drops the other rows.

use crate::auth::Install;
use crate::{db, hub, ApiError, ApiResult, AppState};
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use typeshelf_core::organizer::{self, OrganizerDoc, KEEP_DAYS};
use typeshelf_protocol::ServerMessage;

/// The largest `PUT /me/organizer` body.
pub const MAX_BODY: usize = 2 * 1024 * 1024;

#[derive(Serialize)]
pub struct Organizer {
    rev: i64,
    doc: OrganizerDoc,
}

#[derive(Deserialize)]
struct Push {
    base_rev: i64,
    doc: OrganizerDoc,
}

pub async fn get(State(s): State<AppState>, install: Install) -> ApiResult<Json<Organizer>> {
    let db = s.db();
    let person = db::person(&db, &install.id)?;
    Ok(Json(current(&db, &person)?))
}

pub async fn put(State(s): State<AppState>, install: Install, body: Body) -> ApiResult<Json<Organizer>> {
    // ponytail: any read error counts as too large; a client whose connection broke never reads the answer.
    let bytes = axum::body::to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "organizer_too_large", "the organizer is over 2 MiB"))?;
    let invalid = |e: String| ApiError::bad_request("invalid_organizer", e);
    let push: Push = serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    let sent = organizer::normalize(&push.doc).map_err(invalid)?;

    let mut db = s.db();
    let person = db::person(&db, &install.id)?;
    let tx = db.transaction()?;
    let stored = current(&tx, &person)?;
    if push.base_rev != stored.rev {
        return Err(ApiError::new(StatusCode::CONFLICT, "organizer_conflict", "the organizer changed on another computer")
            .with("rev", stored.rev)
            .with("doc", &stored.doc));
    }
    let now = db::now();
    // Merged again here, so a client that skipped its own merge loses nothing.
    let doc = organizer::prune(&organizer::merge(&stored.doc, &sent), &now, KEEP_DAYS);
    let rev = stored.rev + 1;
    tx.execute(
        "INSERT INTO organizer_docs (install_id, rev, doc_json, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (install_id) DO UPDATE SET rev = excluded.rev, doc_json = excluded.doc_json, updated_at = excluded.updated_at",
        params![person, rev, serde_json::to_string(&doc)?, now],
    )?;
    tx.execute(&format!("DELETE FROM organizer_docs WHERE install_id IN {} AND install_id <> ?1", db::GROUP), [&person])?;
    tx.commit()?;
    hub::notify_person(&db, &s.hub, &install.id, Some(&install.id), &ServerMessage::OrganizerChanged { rev })?;
    Ok(Json(Organizer { rev, doc }))
}

/// The person's rev (0 without a row) and the pruned merge of every row in the group.
fn current(c: &Connection, person: &str) -> ApiResult<Organizer> {
    let rev = c.query_row("SELECT rev FROM organizer_docs WHERE install_id = ?1", [person], |r| r.get(0)).optional()?.unwrap_or(0);
    let mut stmt = c.prepare(&format!("SELECT doc_json FROM organizer_docs WHERE install_id IN {}", db::GROUP))?;
    let mut doc = OrganizerDoc::default();
    for json in stmt.query_map([person], |r| r.get::<_, String>(0))? {
        doc = organizer::merge(&doc, &serde_json::from_str(&json?)?);
    }
    Ok(Organizer { rev, doc: organizer::prune(&doc, &db::now(), KEEP_DAYS) })
}
