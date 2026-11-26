//! "I need this font" requests: recorded from desktop events, listed and closed by the admin.

use crate::auth::Admin;
use crate::db::{self, GROUP};
use crate::{audit, ApiError, ApiResult, AppState, Page, Paging};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::json;
use typeshelf_protocol::{FONT_REQUEST_NAME_MAX, FONT_REQUEST_NOTE_MAX};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/requests", get(list))
        .route("/requests/{id}/done", post(done))
        .route("/requests/{id}/reopen", post(reopen))
}

/// Open requests one person (an install and everything merged into it) can have; later ones only reach the timeline.
const OPEN_MAX: i64 = 50;

fn cut(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect::<String>().trim_end().to_string()
}

/// Stores a request (unless that person already has the same font open, or `OPEN_MAX` open) and adds it to the
/// timeline. Text past the limits is cut; an empty name is ignored.
pub fn record(c: &Connection, install_id: &str, name: &str, note: Option<&str>, at: &str) -> rusqlite::Result<()> {
    let name = cut(name, FONT_REQUEST_NAME_MAX);
    if name.is_empty() {
        return Ok(());
    }
    let note = note.map(|n| cut(n, FONT_REQUEST_NOTE_MAX)).filter(|n| !n.is_empty());
    let person: String = c.query_row("SELECT COALESCE(merged_into, id) FROM installs WHERE id = ?1", [install_id], |r| r.get(0))?;
    c.execute(
        &format!(
            "INSERT INTO font_requests (install_id, font_name, note, at) SELECT ?2, ?3, ?4, ?5
             WHERE NOT EXISTS (SELECT 1 FROM font_requests WHERE install_id IN {GROUP} AND done_at IS NULL AND lower(font_name) = lower(?3))
               AND (SELECT COUNT(*) FROM font_requests WHERE install_id IN {GROUP} AND done_at IS NULL) < ?6"
        ),
        params![person, install_id, name, note, at, OPEN_MAX],
    )?;
    let payload = json!({ "kind": "font_requested", "name": name, "note": note }).to_string();
    db::insert_event(c, install_id, "request", "user", &format!("Asked for {name}"), &payload, at)
}

/// One request; `install_id`/`install_name` are the person it belongs to now (the merge primary).
#[derive(Serialize)]
struct Request {
    id: i64,
    install_id: String,
    install_name: String,
    font_name: String,
    note: Option<String>,
    at: String,
    done_at: Option<String>,
}

/// `p` = the person a request `r` belongs to now (the merge primary); removed people (`p.revoked_at`) are left out.
const PERSON: &str = "JOIN installs i ON i.id = r.install_id JOIN installs p ON p.id = COALESCE(i.merged_into, i.id)";

#[derive(Deserialize)]
struct ListQuery {
    state: Option<String>,
    sort: Option<String>,
    dir: Option<String>,
}

/// Newest first; `state` = all (default), open or done.
async fn list(State(s): State<AppState>, _: Admin, Query(q): Query<ListQuery>, Query(paging): Query<Paging>) -> ApiResult<Json<Page<Request>>> {
    let state = q.state.unwrap_or_else(|| "all".into());
    if !["all", "open", "done"].contains(&state.as_str()) {
        return Err(ApiError::bad_request("invalid_state", format!("unknown state `{state}`")));
    }
    let order = match crate::sort_dir(q.sort.as_deref(), q.dir.as_deref(), &[("at", true), ("font", false)])? {
        Some((key, desc)) => {
            let d = if desc { "DESC" } else { "ASC" };
            if key == "font" { format!("r.font_name COLLATE NOCASE {d}, r.at DESC, r.id DESC") } else { format!("r.at {d}, r.id {d}") }
        }
        None => "r.at DESC, r.id DESC".to_string(),
    };
    let (offset, limit) = paging.window(Paging::DEFAULT, Paging::MAX);
    let filter = "(?1 = 'all' OR (?1 = 'open') = (r.done_at IS NULL)) AND p.revoked_at IS NULL";
    let db = s.db();
    let total: i64 = db.query_row(&format!("SELECT COUNT(*) FROM font_requests r {PERSON} WHERE {filter}"), [&state], |r| r.get(0))?;
    let mut stmt = db.prepare(&format!(
        "SELECT r.id, p.id, p.name, r.font_name, r.note, r.at, r.done_at FROM font_requests r {PERSON}
         WHERE {filter} ORDER BY {order} LIMIT ?2 OFFSET ?3"
    ))?;
    let items = stmt
        .query_map(params![state, limit, offset], |r| {
            Ok(Request {
                id: r.get(0)?,
                install_id: r.get(1)?,
                install_name: r.get(2)?,
                font_name: r.get(3)?,
                note: r.get(4)?,
                at: r.get(5)?,
                done_at: r.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(Json(Page { items, total }))
}

fn set_done(s: &AppState, admin: &Admin, id: i64, done: bool) -> ApiResult<StatusCode> {
    let db = s.db();
    let changed = db.execute(
        "UPDATE font_requests SET done_at = CASE WHEN ?2 THEN COALESCE(done_at, ?3) END WHERE id = ?1",
        params![id, done, db::now()],
    )?;
    if changed == 0 {
        return Err(ApiError::not_found(format!("no request {id}")));
    }
    let name: String = db.query_row("SELECT font_name FROM font_requests WHERE id = ?1", [id], |r| r.get(0))?;
    audit::record(&db, &admin.email, if done { "font_request_done" } else { "font_request_reopened" }, &id.to_string(), &name)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn done(State(s): State<AppState>, admin: Admin, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    set_done(&s, &admin, id, true)
}

async fn reopen(State(s): State<AppState>, admin: Admin, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    set_done(&s, &admin, id, false)
}

/// For the overview's attention block.
pub fn open_count(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row(&format!("SELECT COUNT(*) FROM font_requests r {PERSON} WHERE r.done_at IS NULL AND p.revoked_at IS NULL"), [], |r| r.get(0))
}
