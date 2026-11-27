//! Admin API, `/api/admin`: sign-in, settings and admin accounts. Installs and library live in their own modules.

use crate::auth::{self, Admin, Owner, SESSION_COOKIE, SESSION_DAYS};
use crate::{audit, db, hub, installs, library, ApiError, ApiResult, AppState};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{AppendHeaders, IntoResponse};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::time::Instant;
use typeshelf_protocol::CommandKind;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/settings/install-key", post(regenerate_key))
        .route("/settings/connection-password", put(set_connection_password))
        .route("/admins", get(list_admins).post(create_admin))
        .route("/admins/{id}", delete(remove_admin))
        .route("/admins/{id}/role", put(set_role))
        .route("/admins/{id}/password", post(reset_password))
        .route("/me/password", put(change_password))
        .merge(audit::routes())
        .merge(installs::routes())
        .merge(library::admin_routes())
        .merge(crate::requests::routes())
        .merge(crate::update::admin_routes())
}

#[derive(Deserialize)]
struct Credentials {
    email: String,
    password: String,
}

/// `Secure` when the request came over HTTPS (a proxy's `X-Forwarded-Proto`) or was addressed to the host of an
/// https public URL. The same server opened by its LAN address over plain HTTP gets a cookie the browser keeps.
fn session_cookie(s: &AppState, headers: &HeaderMap, token: &str, max_age: i64) -> ApiResult<String> {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let proxied_https = header("x-forwarded-proto").is_some_and(|p| p.trim().eq_ignore_ascii_case("https"));
    let url = db::settings(&s.db())?.server_url.to_ascii_lowercase();
    // ponytail: compares the URL's authority as written; enough for `https://host[:port][/path]`.
    let public_host = url.strip_prefix("https://").map(|rest| rest.split('/').next().unwrap_or_default().trim_end_matches(":443").to_string());
    let host = header("host").map(|h| h.trim().to_ascii_lowercase().trim_end_matches(":443").to_string());
    let secure = proxied_https || (public_host.is_some() && public_host == host);
    let flag = if secure { "; Secure" } else { "" };
    Ok(format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}{flag}"))
}

fn too_many_attempts(email: &str, s: &AppState) -> ApiResult<()> {
    if let Some(left) = s.login_throttle.locked_for(email, Instant::now()) {
        let minutes = left.as_secs().div_ceil(60);
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            format!("too many failed sign-ins for this email; try again in {minutes} minutes"),
        ));
    }
    Ok(())
}

/// Stores a new session; the caller has already built the cookie (building it locks the database too).
fn insert_session(c: &Connection, admin_id: i64, token: &str) -> rusqlite::Result<()> {
    c.execute("DELETE FROM sessions WHERE expires_at <= ?1", [db::now()])?;
    c.execute(
        "INSERT INTO sessions (token_hash, admin_id, expires_at) VALUES (?1, ?2, ?3)",
        params![auth::token_hash(token), admin_id, db::ts(Utc::now() + Duration::days(SESSION_DAYS))],
    )?;
    Ok(())
}

async fn login(State(s): State<AppState>, headers: HeaderMap, Json(c): Json<Credentials>) -> ApiResult<impl IntoResponse> {
    too_many_attempts(&c.email, &s)?;
    let found: Option<(i64, String, String)> = s
        .db()
        .query_row("SELECT id, email, password_hash FROM admins WHERE email = ?1", [c.email.trim()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    let Some((admin_id, email)) = found.as_ref().filter(|(_, _, hash)| auth::verify_password(&c.password, hash)).map(|(id, e, _)| (*id, e.clone())) else {
        // Logged once per lock and only for real admins, so failed sign-ins cannot flood the audit log.
        if s.login_throttle.failed(&c.email, Instant::now()) {
            if let Some((_, email, _)) = &found {
                let minutes = auth::LOGIN_WINDOW.as_secs() / 60;
                audit::record(&s.db(), email, "sign_in_failed", email, &format!("{} failed sign-ins; locked for {minutes} minutes", auth::LOGIN_MAX_FAILURES))?;
            }
        }
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "bad_credentials", "wrong email or password"));
    };
    s.login_throttle.clear(&c.email);
    let token = auth::random_hex(32);
    let cookie = session_cookie(&s, &headers, &token, SESSION_DAYS * 86_400)?; // before `s.db()` below: it locks the database too
    let db = s.db();
    insert_session(&db, admin_id, &token)?;
    db.execute("UPDATE admins SET last_login = ?1 WHERE id = ?2", params![db::now(), admin_id])?;
    audit::record(&db, &email, "sign_in", "", "")?;
    Ok((StatusCode::NO_CONTENT, AppendHeaders([(header::SET_COOKIE, cookie)])))
}

async fn logout(State(s): State<AppState>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    if let Some(token) = auth::session_token(&headers) {
        s.db().execute("DELETE FROM sessions WHERE token_hash = ?1", [auth::token_hash(&token)])?;
    }
    Ok((StatusCode::NO_CONTENT, AppendHeaders([(header::SET_COOKIE, session_cookie(&s, &headers, "", 0)?)])))
}

async fn me(admin: Admin) -> Json<Value> {
    Json(json!({ "email": admin.email, "role": admin.role }))
}

/// Settings as the admin UI sees them: the password hash is replaced by a flag.
fn public_settings(s: &db::Settings) -> Value {
    let mut v = serde_json::to_value(s).expect("settings serialize");
    let o = v.as_object_mut().expect("settings is an object");
    o.remove("connection_password_hash");
    o.insert("connection_password_set".into(), json!(!s.connection_password_hash.is_empty()));
    v
}

async fn get_settings(State(s): State<AppState>, _: Admin) -> ApiResult<Json<Value>> {
    Ok(Json(public_settings(&db::settings(&s.db())?)))
}

/// Partial update; `install_key` is ignored here (it has its own endpoint). Pushes the new policy to every install.
async fn put_settings(State(s): State<AppState>, admin: Admin, Json(patch): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    let mut db = s.db();
    let tx = db.transaction()?;
    let old = db::settings(&tx)?;
    let mut merged = serde_json::to_value(&old)?;
    let fields = merged.as_object_mut().expect("settings is an object");
    for (k, v) in patch {
        if matches!(k.as_str(), "install_key" | "connection_password_hash" | "connection_password_set") {
            continue;
        }
        if !fields.contains_key(&k) {
            return Err(ApiError::bad_request("unknown_setting", format!("unknown setting `{k}`")));
        }
        fields.insert(k, v);
    }
    let mut new: db::Settings =
        serde_json::from_value(merged).map_err(|e| ApiError::bad_request("invalid_setting", e.to_string()))?;
    new.brand_name = new.brand_name.trim().to_string();
    if new.brand_name.chars().count() > 40 || new.brand_name.chars().any(char::is_control) {
        return Err(ApiError::bad_request("invalid_setting", "brand_name must be at most 40 characters"));
    }
    // Goes into every desktop's stylesheet, so only a plain hex colour gets through.
    let hex = new.brand_color.strip_prefix('#').unwrap_or("!");
    if !new.brand_color.is_empty() && !(hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err(ApiError::bad_request("invalid_setting", "brand_color must look like #1a2b3c, or be empty"));
    }
    if !new.brand_logo.is_empty() && !valid_logo(&new.brand_logo) {
        return Err(ApiError::bad_request("invalid_setting", "brand_logo must be a PNG, JPEG, WebP or SVG image of at most 200 KB"));
    }
    const DAYS: std::ops::RangeInclusive<u32> = 1..=3650;
    if !DAYS.contains(&new.name_change_days) {
        return Err(ApiError::bad_request("invalid_setting", "name_change_days must be between 1 and 3650"));
    }
    if new.timeline_retention_days.is_some_and(|d| !DAYS.contains(&d)) {
        return Err(ApiError::bad_request("invalid_setting", "timeline_retention_days must be between 1 and 3650, or null to keep everything"));
    }
    db::save_settings(&tx, &new)?;
    if new.policy() != old.policy() {
        for id in db::active_install_ids(&tx)? {
            hub::queue(&tx, &s.hub, &id, CommandKind::PolicyChanged { policy: new.policy() })?;
        }
    }
    // `install_key` never changes here, so it never reaches the log.
    let (before, after) = (serde_json::to_value(&old)?, serde_json::to_value(&new)?);
    let changed: Vec<String> = after
        .as_object()
        .expect("settings is an object")
        .iter()
        .filter(|(k, v)| !matches!(k.as_str(), "install_key" | "connection_password_hash") && before.get(k.as_str()) != Some(v))
        // The logo is a long data URL: the log only says that it changed.
        .map(|(k, v)| match k.as_str() {
            "brand_logo" => format!("brand_logo: {}", if new.brand_logo.is_empty() { "removed" } else { "changed" }),
            _ => format!("{k}: {} → {v}", before[k.as_str()]),
        })
        .collect();
    if !changed.is_empty() {
        audit::record(&tx, &admin.email, "settings_changed", "", &changed.join(", "))?;
    }
    tx.commit()?;
    Ok(Json(public_settings(&new)))
}

/// Base64 image data URL, at most [`MAX_LOGO`] characters. Only base64 characters after the prefix, so the value
/// can't break out of the `src` attribute it ends up in on every desktop.
const MAX_LOGO: usize = 200 * 1024;
fn valid_logo(s: &str) -> bool {
    let data = ["png", "jpeg", "webp", "svg+xml"].iter().find_map(|t| s.strip_prefix(&format!("data:image/{t};base64,")));
    s.len() <= MAX_LOGO && data.is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)))
}

/// A custom install key: 8–128 URL-unreserved characters, so it travels unescaped in the `X-Install-Key` header
/// and in a `?key=` query alike.
fn valid_install_key(k: &str) -> bool {
    (8..=128).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~".contains(&b))
}

/// No body: a new random key. `{ "install_key": "…" }`: that key. Either way computers set up with the old key are
/// refused until they use the new one.
async fn regenerate_key(State(s): State<AppState>, Owner(admin): Owner, body: axum::body::Bytes) -> ApiResult<Json<Value>> {
    let custom = if body.iter().all(u8::is_ascii_whitespace) {
        None
    } else {
        let bad = || ApiError::bad_request("invalid_install_key", "The install key must be 8 to 128 characters: letters, digits, - _ . or ~, no spaces.");
        let v: Value = serde_json::from_slice(&body).map_err(|_| bad())?;
        let key = v.get("install_key").and_then(Value::as_str).filter(|k| valid_install_key(k)).ok_or_else(bad)?;
        Some(key.to_string())
    };
    let mut db = s.db();
    let tx = db.transaction()?;
    let mut settings = db::settings(&tx)?;
    let action = if custom.is_some() { "install_key_set" } else { "install_key_regenerated" };
    settings.install_key = custom.unwrap_or_else(|| auth::random_hex(16));
    db::save_settings(&tx, &settings)?;
    audit::record(&tx, &admin.email, action, "", "")?;
    tx.commit()?;
    Ok(Json(json!({ "install_key": settings.install_key })))
}

/// `null` or `""` clears the unlock password. The hash is computed before the database lock is taken (argon2 is slow).
async fn set_connection_password(State(s): State<AppState>, Owner(admin): Owner, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let bad = || ApiError::bad_request("invalid_password", "The unlock password must be 8 to 128 characters.");
    let hash = match body.get("password") {
        None => return Err(bad()),
        Some(Value::Null) => String::new(),
        Some(Value::String(p)) if p.is_empty() => String::new(),
        Some(Value::String(p)) if (8..=128).contains(&p.chars().count()) => auth::hash_password(p),
        Some(_) => return Err(bad()),
    };
    let mut db = s.db();
    let tx = db.transaction()?;
    let mut settings = db::settings(&tx)?;
    settings.connection_password_hash = hash;
    db::save_settings(&tx, &settings)?;
    for id in db::active_install_ids(&tx)? {
        hub::queue(&tx, &s.hub, &id, CommandKind::PolicyChanged { policy: settings.policy() })?;
    }
    let set = !settings.connection_password_hash.is_empty();
    audit::record(&tx, &admin.email, if set { "connection_password_set" } else { "connection_password_cleared" }, "", "")?;
    tx.commit()?;
    Ok(Json(json!({ "connection_password_set": set })))
}

async fn list_admins(State(s): State<AppState>, _: Admin) -> ApiResult<Json<Vec<Value>>> {
    let db = s.db();
    let mut stmt = db.prepare("SELECT id, email, role, last_login FROM admins ORDER BY email")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(json!({ "id": r.get::<_, i64>(0)?, "email": r.get::<_, String>(1)?, "role": r.get::<_, String>(2)?,
                       "last_login": r.get::<_, Option<String>>(3)? }))
        })?
        .collect::<Result<_, _>>()?;
    Ok(Json(rows))
}

pub const MIN_PASSWORD: usize = 8;

fn check_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < MIN_PASSWORD {
        return Err(ApiError::bad_request("weak_password", format!("passwords need at least {MIN_PASSWORD} characters")));
    }
    Ok(())
}

/// Sets an admin's password and ends all of their sessions (shared with the `create-admin` CLI).
pub fn set_password(c: &Connection, admin_id: i64, password_hash: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE admins SET password_hash = ?1 WHERE id = ?2", params![password_hash, admin_id])?;
    c.execute("DELETE FROM sessions WHERE admin_id = ?1", [admin_id])?;
    Ok(())
}

/// `(email, role)` of an admin, or 404.
fn find_admin(c: &Connection, id: i64) -> ApiResult<(String, String)> {
    c.query_row("SELECT email, role FROM admins WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| ApiError::not_found("no such admin"))
}

fn last_owner() -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "last_owner", "this is the only owner; make another admin an owner first")
}

fn owner_count(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM admins WHERE role = 'owner'", [], |r| r.get(0))
}

/// Removing an admin ends their sessions (they cascade with the row).
async fn remove_admin(State(s): State<AppState>, Owner(me): Owner, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let mut db = s.db();
    let tx = db.transaction()?;
    let (email, role) = find_admin(&tx, id)?;
    if role == "owner" && owner_count(&tx)? <= 1 {
        return Err(last_owner());
    }
    tx.execute("DELETE FROM sessions WHERE admin_id = ?1", [id])?;
    tx.execute("DELETE FROM admins WHERE id = ?1", [id])?;
    audit::record(&tx, &me.email, "admin_removed", &email, "")?;
    tx.commit()?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct RoleBody {
    role: String,
}

async fn set_role(State(s): State<AppState>, Owner(me): Owner, Path(id): Path<i64>, Json(body): Json<RoleBody>) -> ApiResult<StatusCode> {
    if !["owner", "admin"].contains(&body.role.as_str()) {
        return Err(ApiError::bad_request("invalid_role", "role must be owner or admin"));
    }
    let mut db = s.db();
    let tx = db.transaction()?;
    let (email, old) = find_admin(&tx, id)?;
    if old == body.role {
        return Ok(StatusCode::NO_CONTENT);
    }
    if old == "owner" && owner_count(&tx)? <= 1 {
        return Err(last_owner());
    }
    tx.execute("UPDATE admins SET role = ?1 WHERE id = ?2", params![body.role, id])?;
    audit::record(&tx, &me.email, "role_changed", &email, &format!("{old} → {}", body.role))?;
    tx.commit()?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct NewPassword {
    password: String,
}

/// An owner sets another admin's password. Hashing runs before the database is locked.
async fn reset_password(State(s): State<AppState>, Owner(me): Owner, Path(id): Path<i64>, Json(body): Json<NewPassword>) -> ApiResult<StatusCode> {
    if id == me.id {
        return Err(ApiError::bad_request("own_password", "change your own password with your current one, under Your password"));
    }
    check_password(&body.password)?;
    let (email, _) = find_admin(&s.db(), id)?;
    let hash = auth::hash_password(&body.password);
    let mut db = s.db();
    let tx = db.transaction()?;
    set_password(&tx, id, &hash)?;
    audit::record(&tx, &me.email, "password_reset", &email, "")?;
    tx.commit()?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct OwnPassword {
    current: String,
    password: String,
}

/// Every admin can change their own password with the current one. All their sessions end, this one included;
/// the response carries a fresh cookie so this browser stays signed in. Wrong guesses count toward the sign-in lock.
async fn change_password(State(s): State<AppState>, me: Admin, headers: HeaderMap, Json(body): Json<OwnPassword>) -> ApiResult<impl IntoResponse> {
    check_password(&body.password)?;
    too_many_attempts(&me.email, &s)?;
    let current: String = s.db().query_row("SELECT password_hash FROM admins WHERE id = ?1", [me.id], |r| r.get(0))?;
    if !auth::verify_password(&body.current, &current) {
        s.login_throttle.failed(&me.email, Instant::now());
        return Err(ApiError::new(StatusCode::FORBIDDEN, "wrong_password", "your current password is not right"));
    }
    let hash = auth::hash_password(&body.password);
    let token = auth::random_hex(32);
    let cookie = session_cookie(&s, &headers, &token, SESSION_DAYS * 86_400)?;
    let mut db = s.db();
    let tx = db.transaction()?;
    set_password(&tx, me.id, &hash)?;
    insert_session(&tx, me.id, &token)?;
    audit::record(&tx, &me.email, "password_changed", &me.email, "")?;
    tx.commit()?;
    Ok((StatusCode::NO_CONTENT, AppendHeaders([(header::SET_COOKIE, cookie)])))
}

/// Shared with the `create-admin` CLI. Returns `false` when the email is taken.
pub fn insert_admin(c: &rusqlite::Connection, email: &str, password_hash: &str, role: &str) -> rusqlite::Result<bool> {
    let n = c.execute(
        "INSERT OR IGNORE INTO admins (email, password_hash, role) VALUES (?1, ?2, ?3)",
        params![email.trim(), password_hash, role],
    )?;
    Ok(n > 0)
}

async fn create_admin(State(s): State<AppState>, Owner(me): Owner, Json(c): Json<Credentials>) -> ApiResult<StatusCode> {
    if !c.email.contains('@') {
        return Err(ApiError::bad_request("invalid_email", "enter an email address"));
    }
    check_password(&c.password)?;
    let hash = auth::hash_password(&c.password);
    let db = s.db();
    if !insert_admin(&db, &c.email, &hash, "admin")? {
        return Err(ApiError::new(StatusCode::CONFLICT, "admin_exists", "an admin with this email already exists"));
    }
    audit::record(&db, &me.email, "admin_added", c.email.trim(), "role admin")?;
    Ok(StatusCode::CREATED)
}
