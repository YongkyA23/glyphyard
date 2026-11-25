//! App updates: one current release in `<data>/update/` (the setup program, its manifest and the manifest's
//! signature, stored exactly as uploaded). The server checks the file against the manifest but cannot check
//! the signature's key: the desktop app does that with the public key baked into its build.

use crate::auth::{Admin, Install, Owner};
use crate::{ApiError, ApiResult, AppState};
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::path::PathBuf;
use typeshelf_core::update::{check_file, Manifest};

const FILE: &str = "setup.exe";
const MANIFEST: &str = "manifest";
const SIGNATURE: &str = "signature";
/// The setup program is about 15 MB; this leaves room without inviting huge uploads.
pub const MAX_UPLOAD: usize = 100 * 1024 * 1024;

// One upload at a time, so the three files on disk always come from the same upload.
static UPLOAD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn desktop_routes() -> Router<AppState> {
    Router::new().route("/update", get(release)).route("/update/file", get(file))
}

/// Any admin sees the current release; only an owner uploads one.
pub fn admin_routes() -> Router<AppState> {
    Router::new().route("/update", get(current).post(upload).layer(DefaultBodyLimit::max(MAX_UPLOAD)))
}

fn dir(s: &AppState) -> PathBuf {
    s.data_dir.join("update")
}

async fn read(s: &AppState, name: &str) -> ApiResult<Option<Vec<u8>>> {
    match tokio::fs::read(dir(s).join(name)).await {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn no_release() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "no_release", "no update has been uploaded")
}

/// `{ manifest, signature }` exactly as uploaded; 404 when there is no release.
async fn release(State(s): State<AppState>, _: Install) -> ApiResult<Json<Value>> {
    let (Some(manifest), Some(signature)) = (read(&s, MANIFEST).await?, read(&s, SIGNATURE).await?) else {
        return Err(no_release());
    };
    // Both were checked to be text on upload.
    Ok(Json(json!({ "manifest": String::from_utf8_lossy(&manifest), "signature": String::from_utf8_lossy(&signature) })))
}

// ponytail: reads the whole setup program into memory (tens of MB); stream it if releases get large.
async fn file(State(s): State<AppState>, _: Install) -> ApiResult<Response> {
    let bytes = read(&s, FILE).await?.ok_or_else(no_release)?;
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_DISPOSITION, "attachment; filename=\"Typeshelf-Setup.exe\"")
        .body(Body::from(bytes))
        .expect("valid response"))
}

/// `{ version, size }` of the current release, or `null`.
async fn current(State(s): State<AppState>, _: Admin) -> ApiResult<Json<Value>> {
    let Some(manifest) = read(&s, MANIFEST).await? else { return Ok(Json(Value::Null)) };
    let m = Manifest::parse(&manifest).map_err(crate::internal)?;
    Ok(Json(json!({ "version": m.version.to_string(), "size": m.size })))
}

/// Multipart `file` (the setup program), `manifest` and `signature` (as written by `update-sign`).
async fn upload(State(s): State<AppState>, Owner(admin): Owner, mut form: Multipart) -> ApiResult<Json<Value>> {
    // Keeps the status, so an upload over `MAX_UPLOAD` is a 413.
    let bad = |e: axum::extract::multipart::MultipartError| ApiError::new(e.status(), "bad_upload", e.body_text());
    let (mut file, mut manifest, mut signature) = (None, None, None);
    while let Some(field) = form.next_field().await.map_err(bad)? {
        let slot = match field.name() {
            Some("file") => &mut file,
            Some("manifest") => &mut manifest,
            Some("signature") => &mut signature,
            _ => continue,
        };
        *slot = Some(field.bytes().await.map_err(bad)?);
    }
    let (Some(file), Some(manifest), Some(signature)) = (file, manifest, signature) else {
        return Err(ApiError::bad_request("missing_field", "send the setup program as `file`, with `manifest` and `signature`"));
    };
    let m = Manifest::parse(&manifest).map_err(|_| ApiError::bad_request("bad_manifest", "the manifest is not one written by `typeshelf-server update-sign`"))?;
    let sig = std::str::from_utf8(&signature).unwrap_or_default().trim();
    if sig.len() != 128 || !sig.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ApiError::bad_request("bad_signature", "the signature is not one written by `typeshelf-server update-sign`"));
    }
    if check_file(&m, &file[..]).is_err() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "file_mismatch", "this setup program does not match the manifest (size or hash differs)"));
    }

    let _one = UPLOAD.lock().await;
    let dir = dir(&s);
    tokio::fs::create_dir_all(&dir).await?;
    for (name, bytes) in [(FILE, &file), (MANIFEST, &manifest), (SIGNATURE, &signature)] {
        tokio::fs::write(dir.join(format!("{name}.new")), bytes).await?;
    }
    // ponytail: three renames, not one atomic swap. A desktop that reads in between gets a mismatched pair,
    // fails verification, discards it and tries again later.
    for name in [FILE, SIGNATURE, MANIFEST] {
        tokio::fs::rename(dir.join(format!("{name}.new")), dir.join(name)).await?;
    }
    tracing::info!(version = %m.version, size = m.size, "update release uploaded");
    crate::audit::record(&s.db(), &admin.email, "release_uploaded", &m.version.to_string(), "")?;
    Ok(Json(json!({ "version": m.version.to_string(), "size": m.size })))
}
