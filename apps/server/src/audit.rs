//! Audit log: who did what, to what, when. Every state-changing admin handler calls `record`.

use crate::auth::Admin;
use crate::{db, ApiResult, AppState, Paging};
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};

/// Writes one row. Pass the open transaction as `c` where the handler has one, so the row commits with the change.
/// `who` is the admin's email; `target` and `detail` must never hold a password, token or the install key.
pub fn record(c: &Connection, who: &str, action: &str, target: &str, detail: &str) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO audit_log (at, admin, action, target, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![db::now(), who, action, target, detail],
    )?;
    Ok(())
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/audit", get(list))
}

#[derive(Deserialize)]
struct Filter {
    admin: Option<String>,
    action: Option<String>,
}

/// Newest first. `admins` and `actions` list every value in the log, for the filter menus.
async fn list(State(s): State<AppState>, _: Admin, Query(f): Query<Filter>, Query(paging): Query<Paging>) -> ApiResult<Json<Value>> {
    let db = s.db();
    let (offset, limit) = paging.window(Paging::DEFAULT, Paging::MAX);
    let who = f.admin.unwrap_or_default().trim().to_string();
    let action = f.action.unwrap_or_default().trim().to_string();
    let filter = "(?1 = '' OR admin = ?1 COLLATE NOCASE) AND (?2 = '' OR action = ?2)";
    let total: i64 = db.query_row(&format!("SELECT COUNT(*) FROM audit_log WHERE {filter}"), params![who, action], |r| r.get(0))?;
    let mut stmt = db.prepare(&format!(
        "SELECT id, at, admin, action, target, detail FROM audit_log WHERE {filter} ORDER BY at DESC, id DESC LIMIT ?3 OFFSET ?4"
    ))?;
    let items: Vec<Value> = stmt
        .query_map(params![who, action, limit, offset], |r| {
            Ok(json!({ "id": r.get::<_, i64>(0)?, "at": r.get::<_, String>(1)?, "admin": r.get::<_, String>(2)?,
                       "action": r.get::<_, String>(3)?, "target": r.get::<_, String>(4)?, "detail": r.get::<_, String>(5)? }))
        })?
        .collect::<Result<_, _>>()?;
    let distinct = |col: &str| -> rusqlite::Result<Vec<String>> {
        let mut stmt = db.prepare(&format!("SELECT DISTINCT {col} FROM audit_log ORDER BY {col} COLLATE NOCASE"))?;
        let values = stmt.query_map([], |r| r.get(0))?.collect();
        values
    };
    Ok(Json(json!({ "items": items, "total": total, "admins": distinct("admin")?, "actions": distinct("action")? })))
}
