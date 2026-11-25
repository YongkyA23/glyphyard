//! Shared library: font files stored under `<data>/library/<hash>`, delivered to installs by the admin.

use crate::auth::{Admin, Install};
use crate::{db, hub, hub::Hub, ApiError, ApiResult, AppState, Page, Paging};
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use typeshelf_core::organize::{self, Action, Change, ScannedFile};
use typeshelf_core::{file_hash, parse_faces};
use typeshelf_protocol::{CommandKind, LibraryFile as WireFile};

const LICENSES: [&str; 4] = ["licensed", "personal", "client", "unknown"];

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/library", get(list).post(upload).layer(DefaultBodyLimit::max(512 * 1024 * 1024)))
        .route("/library/organize", get(organize_plan))
        .route("/library/organize/apply", post(organize_apply))
        .route("/library/{hash}", put(set_license))
        .route("/library/{hash}/block", post(block))
        .route("/library/{hash}/deliver", post(deliver))
}

#[derive(Debug, Serialize)]
pub struct LibraryFile {
    hash: String,
    filename: String,
    ps_name: String,
    family: String,
    style: String,
    version: String,
    license: String,
    size: i64,
    /// `"all"`, `"none"` or a list of install ids.
    delivered_to: Value,
    active_on: i64,
    blocked: bool,
    uploaded_at: String,
}

const COLUMNS: &str = "hash, filename, ps_name, family, style, version, license, size, blocked, uploaded_at";

fn from_row(r: &Row) -> rusqlite::Result<LibraryFile> {
    Ok(LibraryFile {
        hash: r.get(0)?,
        filename: r.get(1)?,
        ps_name: r.get(2)?,
        family: r.get(3)?,
        style: r.get(4)?,
        version: r.get(5)?,
        license: r.get(6)?,
        size: r.get(7)?,
        blocked: r.get(8)?,
        uploaded_at: r.get(9)?,
        delivered_to: Value::Null,
        active_on: 0,
    })
}

fn complete(c: &Connection, mut f: LibraryFile) -> rusqlite::Result<LibraryFile> {
    let mut stmt = c.prepare("SELECT target FROM deliveries WHERE hash = ?1 ORDER BY target")?;
    let targets: Vec<String> = stmt.query_map([&f.hash], |r| r.get(0))?.collect::<Result<_, _>>()?;
    f.delivered_to = if targets.iter().any(|t| t == "all") {
        json!("all")
    } else if targets.is_empty() {
        json!("none")
    } else {
        json!(targets)
    };
    f.active_on = c.query_row(
        "SELECT COUNT(DISTINCT install_id) FROM font_states WHERE hash = ?1 AND active = 1",
        [&f.hash],
        |r| r.get(0),
    )?;
    Ok(f)
}

fn load(c: &Connection, hash: &str) -> ApiResult<LibraryFile> {
    let f = c
        .query_row(&format!("SELECT {COLUMNS} FROM library_files WHERE hash = ?1"), [hash], from_row)
        .optional()?
        .ok_or_else(|| ApiError::not_found("no such library file"))?;
    Ok(complete(c, f)?)
}

fn wire(f: &LibraryFile) -> WireFile {
    WireFile {
        hash: f.hash.clone(),
        filename: f.filename.clone(),
        size: f.size as u64,
        ps_name: f.ps_name.clone(),
        family: f.family.clone(),
        license: f.license.clone(),
        blocked: f.blocked,
    }
}

#[derive(Deserialize)]
struct ListQuery {
    q: Option<String>,
}

/// `q` matches family, style, PostScript name or file name.
async fn list(State(s): State<AppState>, _: Admin, Query(q): Query<ListQuery>, Query(paging): Query<Paging>) -> ApiResult<Json<Page<LibraryFile>>> {
    let db = s.db();
    let (offset, limit) = paging.window(Paging::DEFAULT, Paging::MAX);
    let needle = q.q.unwrap_or_default().trim().to_lowercase();
    let filter = "instr(lower(family || ' ' || style || ' ' || ps_name || ' ' || filename), ?1) > 0";
    let total: i64 = db.query_row(&format!("SELECT COUNT(*) FROM library_files WHERE {filter}"), [&needle], |r| r.get(0))?;
    let mut stmt = db.prepare(&format!(
        "SELECT {COLUMNS} FROM library_files WHERE {filter} ORDER BY family COLLATE NOCASE, style, hash LIMIT ?2 OFFSET ?3"
    ))?;
    let rows: Vec<LibraryFile> = stmt.query_map(params![needle, limit, offset], from_row)?.collect::<Result<_, _>>()?;
    let items = rows.into_iter().map(|f| complete(&db, f)).collect::<Result<_, _>>()?;
    Ok(Json(Page { items, total }))
}

fn store_dir(s: &AppState) -> PathBuf {
    s.data_dir.join("library")
}

/// Library file names are unique so organize plans (which work on names) map back to one file.
fn unique_filename(c: &Connection, name: &str, hash: &str) -> rusqlite::Result<String> {
    let taken: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM library_files WHERE filename = ?1 AND hash != ?2)",
        params![name, hash],
        |r| r.get(0),
    )?;
    if !taken {
        return Ok(name.to_string());
    }
    let p = std::path::Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("font");
    let ext = p.extension().and_then(|e| e.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    Ok(format!("{stem} ({}){ext}", &hash[..8]))
}

async fn upload(State(s): State<AppState>, _: Admin, mut form: Multipart) -> ApiResult<Json<Vec<LibraryFile>>> {
    let bad = |e: axum::extract::multipart::MultipartError| ApiError::bad_request("bad_upload", e.body_text());
    let mut files = Vec::new();
    while let Some(field) = form.next_field().await.map_err(bad)? {
        if field.name() != Some("files") {
            continue;
        }
        let raw_name = field.file_name().unwrap_or("font").to_string();
        let name = organize::sanitize(raw_name.rsplit(['/', '\\']).next().unwrap_or("font"));
        let bytes = field.bytes().await.map_err(bad)?;
        let face = parse_faces(&bytes)
            .map_err(|_| ApiError::bad_request("invalid_font", format!("{raw_name} is not a valid font file")).with("filename", &raw_name))?
            .swap_remove(0);
        files.push((name, bytes, face));
    }
    if files.is_empty() {
        return Err(ApiError::bad_request("no_files", "send one or more font files in the `files` field"));
    }

    let dir = store_dir(&s);
    tokio::fs::create_dir_all(&dir).await?;
    let mut stored = Vec::new();
    for (name, bytes, face) in files {
        let hash = file_hash(&bytes);
        let path = dir.join(&hash);
        if !tokio::fs::try_exists(&path).await? {
            tokio::fs::write(&path, &bytes).await?;
        }
        stored.push((name, hash, bytes.len() as i64, face));
    }

    let db = s.db();
    let now = db::now();
    let mut added = Vec::new();
    for (name, hash, size, face) in stored {
        let filename = unique_filename(&db, &name, &hash)?;
        db.execute(
            "INSERT OR IGNORE INTO library_files (hash, filename, size, ps_name, family, style, version, uploaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![hash, filename, size, face.ps_name, face.family, face.style, face.version, now],
        )?;
        added.push(load(&db, &hash)?);
    }
    Ok(Json(added))
}

#[derive(Deserialize)]
struct LicenseBody {
    license: String,
}

async fn set_license(State(s): State<AppState>, _: Admin, Path(hash): Path<String>, Json(body): Json<LicenseBody>) -> ApiResult<Json<LibraryFile>> {
    if !LICENSES.contains(&body.license.as_str()) {
        return Err(ApiError::bad_request("invalid_license", "license must be licensed, personal, client or unknown"));
    }
    let db = s.db();
    db.execute("UPDATE library_files SET license = ?1 WHERE hash = ?2", params![body.license, hash])?;
    Ok(Json(load(&db, &hash)?))
}

#[derive(Deserialize)]
struct BlockBody {
    blocked: bool,
}

async fn block(State(s): State<AppState>, _: Admin, Path(hash): Path<String>, Json(body): Json<BlockBody>) -> ApiResult<Json<LibraryFile>> {
    let mut db = s.db();
    let tx = db.transaction()?;
    let file = load(&tx, &hash)?;
    tx.execute("UPDATE library_files SET blocked = ?1 WHERE hash = ?2", params![body.blocked, hash])?;
    for id in db::active_install_ids(&tx)? {
        let kind = if body.blocked {
            CommandKind::Block { ps_name: file.ps_name.clone(), hash: Some(hash.clone()) }
        } else {
            CommandKind::Unblock { ps_name: file.ps_name.clone() }
        };
        hub::queue(&tx, &s.hub, &id, kind)?;
    }
    let file = load(&tx, &hash)?;
    tx.commit()?;
    Ok(Json(file))
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Target {
    Keyword(String),
    Installs(Vec<String>),
}

#[derive(Deserialize)]
struct DeliverBody {
    target: Target,
    /// Defaults to the `activate_delivered` setting.
    activate: Option<bool>,
}

async fn deliver(State(s): State<AppState>, _: Admin, Path(hash): Path<String>, Json(body): Json<DeliverBody>) -> ApiResult<Json<LibraryFile>> {
    let mut db = s.db();
    let tx = db.transaction()?;
    let file = load(&tx, &hash)?;
    if file.blocked {
        return Err(ApiError::new(StatusCode::CONFLICT, "blocked", "a blocked font cannot be delivered"));
    }
    let activate = body.activate.unwrap_or(db::settings(&tx)?.activate_delivered);
    let (targets, recipients) = match body.target {
        Target::Keyword(k) if k == "all" => (vec!["all".to_string()], db::active_install_ids(&tx)?),
        Target::Keyword(_) => return Err(ApiError::bad_request("invalid_target", "target must be \"all\" or a list of install ids")),
        Target::Installs(ids) => {
            for id in &ids {
                db::install_name(&tx, id)?.ok_or_else(|| ApiError::not_found(format!("no install {id}")))?;
            }
            (ids.clone(), ids)
        }
    };
    for t in &targets {
        tx.execute(
            "INSERT INTO deliveries (hash, target, activate) VALUES (?1, ?2, ?3)
             ON CONFLICT(hash, target) DO UPDATE SET activate = excluded.activate",
            params![hash, t, activate],
        )?;
    }
    for id in recipients {
        hub::queue(&tx, &s.hub, &id, CommandKind::Deliver { file: wire(&file), activate })?;
    }
    let file = load(&tx, &hash)?;
    tx.commit()?;
    Ok(Json(file))
}

/// Commands a newly registered install needs: current blocks, and (if enabled) everything delivered to all.
pub fn welcome_commands(c: &Connection, hub: &Hub, install_id: &str, deliver_to_new: bool) -> rusqlite::Result<()> {
    let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM library_files WHERE blocked = 1"))?;
    let blocked: Vec<LibraryFile> = stmt.query_map([], from_row)?.collect::<Result<_, _>>()?;
    for f in blocked {
        hub::queue(c, hub, install_id, CommandKind::Block { ps_name: f.ps_name, hash: Some(f.hash) })?;
    }
    if !deliver_to_new {
        return Ok(());
    }
    let mut stmt = c.prepare(&format!(
        "SELECT {COLUMNS}, d.activate FROM library_files JOIN deliveries d USING (hash) WHERE d.target = 'all' AND blocked = 0"
    ))?;
    let delivered: Vec<(LibraryFile, bool)> = stmt.query_map([], |r| Ok((from_row(r)?, r.get(10)?)))?.collect::<Result<_, _>>()?;
    for (f, activate) in delivered {
        hub::queue(c, hub, install_id, CommandKind::Deliver { file: wire(&f), activate })?;
    }
    Ok(())
}

/// Delivered to all, to this install, or to the install it is merged into (deliveries fan out to the group).
const DELIVERED_TO_ME: &str = "EXISTS(SELECT 1 FROM deliveries d WHERE d.hash = library_files.hash
    AND d.target IN ('all', ?1, (SELECT merged_into FROM installs WHERE id = ?1)))";

pub async fn my_library(State(s): State<AppState>, install: Install) -> ApiResult<Json<Vec<WireFile>>> {
    let db = s.db();
    let mut stmt = db.prepare(&format!("SELECT {COLUMNS} FROM library_files WHERE {DELIVERED_TO_ME} ORDER BY family, style"))?;
    let files: Vec<LibraryFile> = stmt.query_map([&install.id], from_row)?.collect::<Result<_, _>>()?;
    Ok(Json(files.iter().map(wire).collect()))
}

/// Font bytes, only when delivered to this install (or to all) and not blocked; otherwise 404.
pub async fn download(State(s): State<AppState>, install: Install, Path(hash): Path<String>) -> ApiResult<Response> {
    let filename: Option<String> = s
        .db()
        .query_row(
            &format!("SELECT filename FROM library_files WHERE hash = ?2 AND blocked = 0 AND {DELIVERED_TO_ME}"),
            params![install.id, hash],
            |r| r.get(0),
        )
        .optional()?;
    let filename = filename.ok_or_else(|| ApiError::not_found("no such font in your library"))?;
    let bytes = tokio::fs::read(store_dir(&s).join(&hash)).await?;
    let disposition = format!("attachment; filename=\"{}\"", filename.replace('"', "_"));
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(Body::from(bytes))
        .expect("valid response"))
}

/// Runs the core planner over the library. Paths are library file names; tidy is off (the store is flat).
async fn library_plan(s: &AppState) -> ApiResult<Vec<Change>> {
    let rows: Vec<(String, String, String)> = {
        let db = s.db();
        let mut stmt = db.prepare("SELECT hash, filename, uploaded_at FROM library_files")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        rows
    };
    let dir = store_dir(s);
    // ponytail: re-reads and re-parses every library file per request; cache faces in the table if the library grows large.
    let changes = tokio::task::spawn_blocking(move || {
        let files: Vec<ScannedFile> = rows
            .into_iter()
            .map(|(hash, filename, uploaded_at)| {
                let faces = std::fs::read(dir.join(&hash)).ok().and_then(|b| parse_faces(&b).ok()).unwrap_or_default();
                ScannedFile { path: filename.into(), hash, modified: db::parse_ts(&uploaded_at).timestamp(), faces, home: None }
            })
            .collect();
        let opts = organize::Options { tidy: false, quarantine_dir: "quarantine".into(), aside_dir: "web".into() };
        organize::plan(&files, &HashSet::new(), &opts)
    })
    .await
    .map_err(crate::internal)?;
    Ok(changes)
}

async fn organize_plan(State(s): State<AppState>, _: Admin) -> ApiResult<Json<Value>> {
    // Advice that changes no file (alternates, groups, skips) is listed apart so it is not counted as work.
    let (changes, notes): (Vec<_>, Vec<_>) = library_plan(&s).await?.into_iter().partition(|c| c.action.changes_files());
    Ok(Json(json!({ "changes": changes, "notes": notes })))
}

/// Moves a stored file to `<data>/quarantine/<filename>`, or `<hash[..8]>-<filename>` when that name is taken.
fn quarantine_file(s: &AppState, hash: &str, filename: &str) -> std::io::Result<()> {
    let dir = s.data_dir.join("quarantine");
    std::fs::create_dir_all(&dir)?;
    let mut to = dir.join(filename);
    if to.try_exists()? {
        to = dir.join(format!("{}-{filename}", &hash[..8]));
    }
    if to.try_exists()? {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("{} already exists", to.display())));
    }
    std::fs::rename(store_dir(s).join(hash), to)
}

/// Applies renames and removals (quarantine/trash move the stored file to `<data>/quarantine/`); grouping,
/// alternates and skips are advice only.
async fn organize_apply(State(s): State<AppState>, _: Admin) -> ApiResult<Json<Value>> {
    let changes = library_plan(&s).await?;
    let name = |p: &std::path::Path| p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let mut applied = 0;
    {
        // ponytail: file moves run under the DB lock; a same-disk rename is quick.
        let db = s.db();
        for change in &changes {
            match &change.action {
                Action::Rename { from, to } => {
                    let free: bool =
                        db.query_row("SELECT NOT EXISTS(SELECT 1 FROM library_files WHERE filename = ?1)", [name(to)], |r| r.get(0))?;
                    if free && db.execute("UPDATE library_files SET filename = ?1 WHERE filename = ?2", params![name(to), name(from)])? > 0 {
                        applied += 1;
                    }
                }
                Action::Trash { path: from, .. } | Action::Quarantine { from, .. } => {
                    let filename = name(from);
                    let hash: Option<String> = db
                        .query_row("SELECT hash FROM library_files WHERE filename = ?1", [&filename], |r| r.get(0))
                        .optional()?;
                    let Some(hash) = hash else { continue };
                    match quarantine_file(&s, &hash, &filename) {
                        Ok(()) => {
                            db.execute("DELETE FROM library_files WHERE hash = ?1", [&hash])?;
                            applied += 1;
                        }
                        Err(e) => tracing::warn!("could not quarantine library file {filename} ({hash}): {e}"),
                    }
                }
                _ => {}
            }
        }
    }
    Ok(Json(json!({ "applied": applied })))
}
