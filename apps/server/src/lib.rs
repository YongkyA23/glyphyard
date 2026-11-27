//! Glyphyard admin server: desktop sync API (`/api/v1`), admin API (`/api/admin`) and the admin UI.

pub mod admin;
pub mod audit;
pub mod auth;
pub mod db;
pub mod desktop;
pub mod hub;
pub mod installs;
pub mod library;
pub mod organizer;
pub mod requests;
pub mod update;

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::{Json, Router};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    // ponytail: one connection behind a mutex, held only for synchronous work (never across `.await`).
    // Fine for an admin server with hundreds of installs; switch to a connection pool + spawn_blocking if it shows up in latency.
    db: Arc<Mutex<Connection>>,
    pub data_dir: PathBuf,
    pub hub: Arc<hub::Hub>,
    pub login_throttle: Arc<auth::LoginThrottle>,
}

impl AppState {
    pub fn new(conn: Connection, data_dir: PathBuf) -> Self {
        Self { db: Arc::new(Mutex::new(conn)), data_dir, hub: Arc::default(), login_throttle: Arc::default() }
    }

    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn app(state: AppState, static_dir: &Path) -> Router {
    let index = static_dir.join("index.html");
    let router = Router::new()
        .nest("/api/v1", desktop::routes())
        .nest("/api/admin", admin::routes());
    let router = if index.exists() {
        router.fallback_service(ServeDir::new(static_dir).fallback(ServeFile::new(index)))
    } else {
        router.fallback(|| async {
            Html("<!doctype html><title>Glyphyard</title><p>Glyphyard server is running. The admin UI is not built yet (<code>apps/server/static</code>).</p>")
        })
    };
    router.layer(TraceLayer::new_for_http()).with_state(state)
}

/// `?offset=&limit=` on admin lists. A separate `Query<Paging>` extractor, next to the endpoint's own query.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct Paging {
    pub offset: Option<u32>,
    pub limit: Option<u32>,
}

/// One page of a list plus the size of the whole (filtered) list.
#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
}

impl Paging {
    pub const DEFAULT: i64 = 50;
    pub const MAX: i64 = 200;

    /// `(offset, limit)` for SQL, with the limit clamped to `1..=max`.
    pub fn window(self, default: i64, max: i64) -> (i64, i64) {
        (self.offset.map_or(0, i64::from), self.limit.map_or(default, i64::from).clamp(1, max))
    }

    /// Pages a list that was filtered in Rust.
    pub fn slice<T>(self, all: Vec<T>) -> Page<T> {
        let (offset, limit) = self.window(Self::DEFAULT, Self::MAX);
        let total = all.len() as i64;
        Page { items: all.into_iter().skip(offset as usize).take(limit as usize).collect(), total }
    }
}

/// `{ "error": code, "message": text, ...extra }` with a status.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    extra: Map<String, Value>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into(), extra: Map::new() }
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn with(mut self, key: &str, value: impl Serialize) -> Self {
        self.extra.insert(key.into(), serde_json::to_value(value).unwrap_or(Value::Null));
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = json!({ "error": self.code, "message": self.message });
        body.as_object_mut().expect("object").extend(self.extra);
        (self.status, Json(body)).into_response()
    }
}

/// Parses `sort`/`dir` against `allowed` (`(key, descending by default)`): `None` without `sort`,
/// else `(key, desc)`. Unknown `sort` or `dir` is `400 invalid_sort`.
pub fn sort_dir(sort: Option<&str>, dir: Option<&str>, allowed: &[(&str, bool)]) -> ApiResult<Option<(String, bool)>> {
    let Some(sort) = sort else { return Ok(None) };
    let Some(&(key, default_desc)) = allowed.iter().find(|(k, _)| *k == sort) else {
        return Err(ApiError::bad_request("invalid_sort", format!("unknown sort `{sort}`")));
    };
    let desc = match dir {
        None => default_desc,
        Some("asc") => false,
        Some("desc") => true,
        Some(other) => return Err(ApiError::bad_request("invalid_sort", format!("unknown dir `{other}`"))),
    };
    Ok(Some((key.to_string(), desc)))
}

pub(crate) fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!("internal error: {e}");
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal server error")
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        internal(e)
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        internal(e)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        internal(e)
    }
}
