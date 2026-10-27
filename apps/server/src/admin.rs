//! Admin API, `/api/admin`: sign-in, settings and admin accounts. Installs and library live in their own modules.

use crate::auth::{self, Admin, SESSION_COOKIE, SESSION_DAYS};
use crate::{db, hub, installs, library, ApiError, ApiResult, AppState};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{AppendHeaders, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use typeshelf_protocol::CommandKind;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/settings/install-key", post(regenerate_key))
        .route("/admins", get(list_admins).post(create_admin))
        .merge(installs::routes())
        .merge(library::admin_routes())
}

#[derive(Deserialize)]
struct Credentials {
    email: String,
    password: String,
}

fn session_cookie(token: &str, max_age: i64) -> String {
    format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}")
}

async fn login(State(s): State<AppState>, Json(c): Json<Credentials>) -> ApiResult<impl IntoResponse> {
    let found: Option<(i64, String)> = s
        .db()
        .query_row("SELECT id, password_hash FROM admins WHERE email = ?1", [c.email.trim()], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let admin_id = found
        .filter(|(_, hash)| auth::verify_password(&c.password, hash))
        .map(|(id, _)| id)
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "bad_credentials", "wrong email or password"))?;
    let token = auth::random_hex(32);
    let db = s.db();
    db.execute("DELETE FROM sessions WHERE expires_at <= ?1", [db::now()])?;
    db.execute(
        "INSERT INTO sessions (token_hash, admin_id, expires_at) VALUES (?1, ?2, ?3)",
        params![auth::token_hash(&token), admin_id, db::ts(Utc::now() + Duration::days(SESSION_DAYS))],
    )?;
    db.execute("UPDATE admins SET last_login = ?1 WHERE id = ?2", params![db::now(), admin_id])?;
    let cookie = session_cookie(&token, SESSION_DAYS * 86_400);
    Ok((StatusCode::NO_CONTENT, AppendHeaders([(header::SET_COOKIE, cookie)])))
}

async fn logout(State(s): State<AppState>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    if let Some(token) = auth::session_token(&headers) {
        s.db().execute("DELETE FROM sessions WHERE token_hash = ?1", [auth::token_hash(&token)])?;
    }
    Ok((StatusCode::NO_CONTENT, AppendHeaders([(header::SET_COOKIE, session_cookie("", 0))])))
}

async fn me(admin: Admin) -> Json<Value> {
    Json(json!({ "email": admin.email, "role": admin.role }))
}

async fn get_settings(State(s): State<AppState>, _: Admin) -> ApiResult<Json<db::Settings>> {
    Ok(Json(db::settings(&s.db())?))
}

/// Partial update; `install_key` is ignored here (it has its own endpoint). Pushes the new policy to every install.
async fn put_settings(State(s): State<AppState>, _: Admin, Json(patch): Json<Map<String, Value>>) -> ApiResult<Json<db::Settings>> {
    let mut db = s.db();
    let tx = db.transaction()?;
    let old = db::settings(&tx)?;
    let mut merged = serde_json::to_value(&old)?;
    let fields = merged.as_object_mut().expect("settings is an object");
    for (k, v) in patch {
        if k == "install_key" {
            continue;
        }
        if !fields.contains_key(&k) {
            return Err(ApiError::bad_request("unknown_setting", format!("unknown setting `{k}`")));
        }
        fields.insert(k, v);
    }
    let new: db::Settings =
        serde_json::from_value(merged).map_err(|e| ApiError::bad_request("invalid_setting", e.to_string()))?;
    if new.name_change_days == 0 {
        return Err(ApiError::bad_request("invalid_setting", "name_change_days must be at least 1"));
    }
    db::save_settings(&tx, &new)?;
    if new.policy() != old.policy() {
        for id in db::active_install_ids(&tx)? {
            hub::queue(&tx, &s.hub, &id, CommandKind::PolicyChanged { policy: new.policy() })?;
        }
    }
    tx.commit()?;
    Ok(Json(new))
}

async fn regenerate_key(State(s): State<AppState>, _: Admin) -> ApiResult<Json<Value>> {
    let db = s.db();
    let mut settings = db::settings(&db)?;
    settings.install_key = auth::random_hex(16);
    db::save_settings(&db, &settings)?;
    Ok(Json(json!({ "install_key": settings.install_key })))
}

async fn list_admins(State(s): State<AppState>, _: Admin) -> ApiResult<Json<Vec<Value>>> {
    let db = s.db();
    let mut stmt = db.prepare("SELECT email, role, last_login FROM admins ORDER BY email")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(json!({ "email": r.get::<_, String>(0)?, "role": r.get::<_, String>(1)?, "last_login": r.get::<_, Option<String>>(2)? }))
        })?
        .collect::<Result<_, _>>()?;
    Ok(Json(rows))
}

pub const MIN_PASSWORD: usize = 8;

/// Shared with the `create-admin` CLI. Returns `false` when the email is taken.
pub fn insert_admin(c: &rusqlite::Connection, email: &str, password_hash: &str, role: &str) -> rusqlite::Result<bool> {
    let n = c.execute(
        "INSERT OR IGNORE INTO admins (email, password_hash, role) VALUES (?1, ?2, ?3)",
        params![email.trim(), password_hash, role],
    )?;
    Ok(n > 0)
}

async fn create_admin(State(s): State<AppState>, _: Admin, Json(c): Json<Credentials>) -> ApiResult<StatusCode> {
    if !c.email.contains('@') {
        return Err(ApiError::bad_request("invalid_email", "enter an email address"));
    }
    if c.password.chars().count() < MIN_PASSWORD {
        return Err(ApiError::bad_request("weak_password", format!("passwords need at least {MIN_PASSWORD} characters")));
    }
    let hash = auth::hash_password(&c.password);
    if !insert_admin(&s.db(), &c.email, &hash, "admin")? {
        return Err(ApiError::new(StatusCode::CONFLICT, "admin_exists", "an admin with this email already exists"));
    }
    Ok(StatusCode::CREATED)
}
