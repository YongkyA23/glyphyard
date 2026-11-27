//! App updates: one current release per platform (the release file, its manifest and the manifest's
//! signature, stored exactly as uploaded). Windows lives in `<data>/update/`, macOS in `<data>/update/macos/`. The server checks the file against the manifest but cannot check
//! the signature's key: the desktop app does that with the public key baked into its build.

use crate::auth::{Admin, Install, Owner};
use crate::{ApiError, ApiResult, AppState};
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Query, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use typeshelf_core::update::{check_file, Manifest, Platform};

const MANIFEST: &str = "manifest";
const SIGNATURE: &str = "signature";
/// A release file is about 15 MB; this leaves room without inviting huge uploads.
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

/// The slot directory and the release file's name in it.
fn slot(s: &AppState, p: Platform) -> (PathBuf, &'static str) {
    match p {
        Platform::Windows => (s.data_dir.join("update"), "setup.exe"),
        Platform::Macos => (s.data_dir.join("update").join("macos"), "app.tar.gz"),
    }
}

/// `?platform=`: absent means Windows (installed apps send nothing); an unknown name has no release.
#[derive(Deserialize)]
struct PlatformQuery {
    platform: Option<String>,
}

impl PlatformQuery {
    fn get(&self) -> ApiResult<Platform> {
        match &self.platform {
            None => Ok(Platform::Windows),
            Some(n) => Platform::from_name(n).ok_or_else(no_release),
        }
    }
}

async fn read(dir: &std::path::Path, name: &str) -> ApiResult<Option<Vec<u8>>> {
    match tokio::fs::read(dir.join(name)).await {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn no_release() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "no_release", "no update has been uploaded")
}

/// `{ manifest, signature }` exactly as uploaded; 404 when there is no release.
async fn release(State(s): State<AppState>, _: Install, Query(q): Query<PlatformQuery>) -> ApiResult<Json<Value>> {
    let (dir, _) = slot(&s, q.get()?);
    let (Some(manifest), Some(signature)) = (read(&dir, MANIFEST).await?, read(&dir, SIGNATURE).await?) else {
        return Err(no_release());
    };
    // Both were checked to be text on upload.
    Ok(Json(json!({ "manifest": String::from_utf8_lossy(&manifest), "signature": String::from_utf8_lossy(&signature) })))
}

// ponytail: reads the whole release file into memory (tens of MB); stream it if releases get large.
async fn file(State(s): State<AppState>, _: Install, Query(q): Query<PlatformQuery>) -> ApiResult<Response> {
    let p = q.get()?;
    let (dir, name) = slot(&s, p);
    let bytes = read(&dir, name).await?.ok_or_else(no_release)?;
    let download = match p {
        Platform::Windows => "Glyphyard-Setup.exe",
        Platform::Macos => "Glyphyard.app.tar.gz",
    };
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{download}\""))
        .body(Body::from(bytes))
        .expect("valid response"))
}

/// `{ windows, macos }`, each `{ version, size }` of that platform's current release, or `null`.
async fn current(State(s): State<AppState>, _: Admin) -> ApiResult<Json<Value>> {
    let mut out = json!({ "windows": null, "macos": null });
    for p in [Platform::Windows, Platform::Macos] {
        let Some(manifest) = read(&slot(&s, p).0, MANIFEST).await? else { continue };
        let m = Manifest::parse(&manifest).map_err(crate::internal)?;
        out[p.as_str()] = json!({ "version": m.version.to_string(), "size": m.size });
    }
    Ok(Json(out))
}

/// Multipart `file` (the release file), `manifest` and `signature` (as written by `update-sign`).
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
        return Err(ApiError::bad_request("missing_field", "send the release file as `file`, with `manifest` and `signature`"));
    };
    let m = Manifest::parse(&manifest).map_err(|_| ApiError::bad_request("bad_manifest", "the manifest is not one written by `typeshelf-server update-sign`"))?;
    let sig = std::str::from_utf8(&signature).unwrap_or_default().trim();
    if sig.len() != 128 || !sig.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ApiError::bad_request("bad_signature", "the signature is not one written by `typeshelf-server update-sign`"));
    }
    if check_file(&m, &file[..]).is_err() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "file_mismatch", "this release file does not match the manifest (size or hash differs)"));
    }

    let _one = UPLOAD.lock().await;
    let (dir, file_name) = slot(&s, m.platform);
    tokio::fs::create_dir_all(&dir).await?;
    for (name, bytes) in [(file_name, &file), (MANIFEST, &manifest), (SIGNATURE, &signature)] {
        tokio::fs::write(dir.join(format!("{name}.new")), bytes).await?;
    }
    // ponytail: three renames, not one atomic swap. A desktop that reads in between gets a mismatched pair,
    // fails verification, discards it and tries again later.
    for name in [file_name, SIGNATURE, MANIFEST] {
        tokio::fs::rename(dir.join(format!("{name}.new")), dir.join(name)).await?;
    }
    tracing::info!(version = %m.version, size = m.size, platform = m.platform.as_str(), "update release uploaded");
    crate::audit::record(&s.db(), &admin.email, "release_uploaded", &m.version.to_string(), m.platform.as_str())?;
    Ok(Json(json!({ "version": m.version.to_string(), "size": m.size, "platform": m.platform.as_str() })))
}
