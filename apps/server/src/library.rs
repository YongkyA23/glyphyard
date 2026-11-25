//! Shared library: font files stored under `<data>/library/<hash>`, delivered to installs by the admin.

use crate::auth::{Admin, Install};
use crate::{audit, db, hub, hub::Hub, installs, ApiError, ApiResult, AppState, Page, Paging};
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
        .route("/library/bulk", post(bulk))
        .route("/library/{hash}", put(set_license).delete(remove))
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

/// The list's search: `?1` (trimmed, lowercased) in family, style, PostScript name or file name.
const SEARCH: &str = "instr(lower(family || ' ' || style || ' ' || ps_name || ' ' || filename), ?1) > 0";

/// `q` matches family, style, PostScript name or file name.
async fn list(State(s): State<AppState>, _: Admin, Query(q): Query<ListQuery>, Query(paging): Query<Paging>) -> ApiResult<Json<Page<LibraryFile>>> {
    let db = s.db();
    let (offset, limit) = paging.window(Paging::DEFAULT, Paging::MAX);
    let needle = q.q.unwrap_or_default().trim().to_lowercase();
    let filter = SEARCH;
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

async fn upload(State(s): State<AppState>, admin: Admin, mut form: Multipart) -> ApiResult<Json<Vec<LibraryFile>>> {
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
        let new = db.execute(
            "INSERT OR IGNORE INTO library_files (hash, filename, size, ps_name, family, style, version, uploaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![hash, filename, size, face.ps_name, face.family, face.style, face.version, now],
        )?;
        if new > 0 {
            audit::record(&db, &admin.email, "font_uploaded", &hash, &filename)?;
        }
        added.push(load(&db, &hash)?);
    }
    Ok(Json(added))
}

#[derive(Deserialize)]
struct LicenseBody {
    license: String,
}

async fn set_license(State(s): State<AppState>, admin: Admin, Path(hash): Path<String>, Json(body): Json<LicenseBody>) -> ApiResult<Json<LibraryFile>> {
    if !LICENSES.contains(&body.license.as_str()) {
        return Err(ApiError::bad_request("invalid_license", "license must be licensed, personal, client or unknown"));
    }
    let db = s.db();
    if db.execute("UPDATE library_files SET license = ?1 WHERE hash = ?2", params![body.license, hash])? > 0 {
        audit::record(&db, &admin.email, "license_changed", &hash, &body.license)?;
    }
    Ok(Json(load(&db, &hash)?))
}

#[derive(Deserialize)]
struct BlockBody {
    blocked: bool,
}

async fn block(State(s): State<AppState>, admin: Admin, Path(hash): Path<String>, Json(body): Json<BlockBody>) -> ApiResult<Json<LibraryFile>> {
    let mut db = s.db();
    let tx = db.transaction()?;
    let file = set_blocked(&tx, &s.hub, &hash, body.blocked)?;
    audit::record(&tx, &admin.email, if body.blocked { "font_blocked" } else { "font_unblocked" }, &hash, &file.ps_name)?;
    tx.commit()?;
    Ok(Json(file))
}

/// Sets `blocked` and pushes Block/Unblock to every install.
fn set_blocked(c: &Connection, hub: &Hub, hash: &str, blocked: bool) -> ApiResult<LibraryFile> {
    let file = load(c, hash)?;
    c.execute("UPDATE library_files SET blocked = ?1 WHERE hash = ?2", params![blocked, hash])?;
    // Desktops unblock by name, which also clears other files with that name: block those again after it.
    let namesakes: Vec<String> = if blocked {
        Vec::new()
    } else {
        let mut stmt = c.prepare("SELECT hash FROM library_files WHERE blocked = 1 AND ps_name = ?1 COLLATE NOCASE ORDER BY hash")?;
        let hashes = stmt.query_map([&file.ps_name], |r| r.get(0))?.collect::<Result<_, _>>()?;
        hashes
    };
    for id in db::active_install_ids(c)? {
        let kind = if blocked {
            CommandKind::Block { ps_name: file.ps_name.clone(), hash: Some(hash.to_string()) }
        } else {
            CommandKind::Unblock { ps_name: file.ps_name.clone() }
        };
        hub::queue(c, hub, &id, kind)?;
        for h in &namesakes {
            hub::queue(c, hub, &id, CommandKind::Block { ps_name: file.ps_name.clone(), hash: Some(h.clone()) })?;
        }
    }
    load(c, hash)
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

async fn deliver(State(s): State<AppState>, admin: Admin, Path(hash): Path<String>, Json(body): Json<DeliverBody>) -> ApiResult<Json<LibraryFile>> {
    let mut db = s.db();
    let tx = db.transaction()?;
    let activate = body.activate.unwrap_or(db::settings(&tx)?.activate_delivered);
    let file = set_delivery(&tx, &s.hub, &hash, &body.target, Some(activate))?;
    let targets = match &body.target {
        Target::Keyword(k) => k.clone(),
        Target::Installs(ids) => ids.join(", "),
    };
    let on = if activate { ", turned on" } else { "" };
    if targets.is_empty() {
        audit::record(&tx, &admin.email, "font_undelivered", &hash, &file.ps_name)?;
    } else {
        audit::record(&tx, &admin.email, "font_delivered", &hash, &format!("{} to {targets}{on}", file.ps_name))?;
    }
    tx.commit()?;
    Ok(Json(file))
}

/// `(delivery targets, installs to send it to)`; unknown or revoked installs are 404.
fn resolve_target(c: &Connection, target: &Target) -> ApiResult<(Vec<String>, Vec<String>)> {
    match target {
        Target::Keyword(k) if k == "all" => Ok((vec!["all".to_string()], db::active_install_ids(c)?)),
        Target::Keyword(_) => Err(ApiError::bad_request("invalid_target", "target must be \"all\" or a list of install ids")),
        Target::Installs(ids) => {
            for id in ids {
                installs::exists(c, id)?;
            }
            Ok((ids.clone(), ids.clone()))
        }
    }
}

/// Replaces the file's targets with `target` and sends it to every recipient. Targets left out no longer get it
/// (new installs, downloads, their library list); computers that already have the file keep it.
fn set_delivery(c: &Connection, hub: &Hub, hash: &str, target: &Target, activate: Option<bool>) -> ApiResult<LibraryFile> {
    let file = load(c, hash)?;
    if file.blocked {
        return Err(ApiError::new(StatusCode::CONFLICT, "blocked", "a blocked font cannot be delivered"));
    }
    let activate = activate.unwrap_or(db::settings(c)?.activate_delivered);
    let (targets, recipients) = resolve_target(c, target)?;
    c.execute("DELETE FROM deliveries WHERE hash = ?1", [hash])?;
    // Unsent Delivers to computers no longer covered (directly or through the install they are merged into) are
    // cancelled. Rows are never deleted: `commands.id` has no AUTOINCREMENT, so a deleted id could come back.
    let mut stmt = c.prepare(
        "SELECT c.id, c.install_id, i.merged_into FROM commands c JOIN installs i ON i.id = c.install_id
         WHERE c.acked_at IS NULL AND c.kind = 'deliver' AND json_extract(c.payload_json, '$.file.hash') = ?1",
    )?;
    let queued: Vec<(i64, String, Option<String>)> = stmt.query_map([hash], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    drop(stmt);
    let covered = |id: &String| targets.iter().any(|t| t == "all" || t == id);
    for (id, install, merged_into) in queued {
        if !covered(&install) && !merged_into.as_ref().is_some_and(covered) {
            c.execute("UPDATE commands SET acked_at = ?1, ok = 0, error = 'cancelled' WHERE id = ?2", params![db::now(), id])?;
        }
    }
    for t in &targets {
        c.execute(
            "INSERT INTO deliveries (hash, target, activate) VALUES (?1, ?2, ?3)
             ON CONFLICT(hash, target) DO UPDATE SET activate = excluded.activate",
            params![hash, t, activate],
        )?;
    }
    for id in recipients {
        hub::queue(c, hub, &id, CommandKind::Deliver { file: wire(&file), activate })?;
    }
    load(c, hash)
}

async fn remove(State(s): State<AppState>, admin: Admin, Path(hash): Path<String>) -> ApiResult<StatusCode> {
    let db = s.db();
    let filename = remove_file(&s, &db, &hash)?;
    audit::record(&db, &admin.email, "font_deleted", &hash, &filename)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Takes a file out of the library: the stored file moves to the quarantine folder (as organize does) and its
/// deliveries go with the row. Desktops keep the copies they have. A blocked file is refused (409 `blocked`): its
/// row is what keeps the block listed, sent to new installs and liftable.
/// Returns the file's name.
fn remove_file(s: &AppState, c: &Connection, hash: &str) -> ApiResult<String> {
    let file = load(c, hash)?;
    if file.blocked {
        return Err(ApiError::new(StatusCode::CONFLICT, "blocked", "unblock this font before deleting it"));
    }
    // ponytail: the move runs under the DB lock, like organize_apply; a same-disk rename is quick.
    match quarantine_file(s, hash, &file.filename) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()), // NotFound: the stored file is already gone
        _ => {}
    }
    c.execute("DELETE FROM library_files WHERE hash = ?1", [hash])?;
    Ok(file.filename)
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum BulkAction {
    Deliver,
    Block,
    Unblock,
    Delete,
}

#[derive(Deserialize)]
struct BulkBody {
    action: BulkAction,
    /// The selected files; without it, every file the list's search `q` matches ("" = the whole library).
    hashes: Option<Vec<String>>,
    q: Option<String>,
    /// For `deliver`, as in the single-file endpoint.
    target: Option<Target>,
    activate: Option<bool>,
}

/// One action over many files. Unknown files, files already blocked/unblocked, and blocked files asked to be
/// delivered or deleted are skipped and counted. A delete that fails for one file is counted as failed and the
/// rest go on (files already moved stay moved).
async fn bulk(State(s): State<AppState>, admin: Admin, Json(body): Json<BulkBody>) -> ApiResult<Json<Value>> {
    let mut db = s.db();
    let hashes: Vec<String> = match (body.hashes, &body.q) {
        (Some(h), _) => h,
        (None, Some(q)) => {
            let mut stmt = db.prepare(&format!("SELECT hash FROM library_files WHERE {SEARCH}"))?;
            let h = stmt.query_map([q.trim().to_lowercase()], |r| r.get(0))?.collect::<Result<_, _>>()?;
            h
        }
        (None, None) => return Err(ApiError::bad_request("no_selection", "send the files' `hashes` or the list filter `q`")),
    };
    let target = match (body.action, body.target) {
        (BulkAction::Deliver, None) => return Err(ApiError::bad_request("invalid_target", "deliver needs a target")),
        (_, t) => t,
    };
    let (mut changed, mut skipped, mut failed) = (0, 0, 0);
    let mut count = |r: ApiResult<bool>, keep_going: bool| -> ApiResult<()> {
        match r {
            Ok(true) => changed += 1,
            Ok(false) => skipped += 1,
            Err(e) if e.code == "not_found" || e.code == "blocked" => skipped += 1,
            Err(e) if keep_going => {
                tracing::warn!("bulk delete: {}", e.message);
                failed += 1
            }
            Err(e) => return Err(e),
        }
        Ok(())
    };
    if body.action == BulkAction::Delete {
        // No transaction: a moved file must not get its row back from a rollback.
        for h in &hashes {
            count(remove_file(&s, &db, h).map(|_| true), true)?;
        }
    } else {
        let tx = db.transaction()?;
        if let Some(t) = &target {
            resolve_target(&tx, t)?; // a bad target fails the whole request, not file by file
        }
        for h in &hashes {
            let r = match (body.action, &target) {
                (BulkAction::Deliver, Some(t)) => set_delivery(&tx, &s.hub, h, t, body.activate).map(|_| true),
                (action, _) => {
                    let want = action == BulkAction::Block;
                    load(&tx, h).and_then(|f| if f.blocked == want { Ok(false) } else { set_blocked(&tx, &s.hub, h, want).map(|_| true) })
                }
            };
            count(r, false)?;
        }
        tx.commit()?;
    }
    let action = match body.action {
        BulkAction::Deliver => "bulk_deliver",
        BulkAction::Block => "bulk_block",
        BulkAction::Unblock => "bulk_unblock",
        BulkAction::Delete => "bulk_delete",
    };
    audit::record(&db, &admin.email, action, "", &format!("{changed} changed, {skipped} skipped, {failed} failed"))?;
    Ok(Json(json!({ "changed": changed, "skipped": skipped, "failed": failed })))
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

/// Moves a stored file to `<data>/quarantine/<filename>`, or `<hash[..8]>-<filename>` when that name is taken, then
/// `<hash[..8]>-2-<filename>`, `-3-`, … (the same file can be uploaded and removed many times).
fn quarantine_file(s: &AppState, hash: &str, filename: &str) -> std::io::Result<()> {
    let dir = s.data_dir.join("quarantine");
    std::fs::create_dir_all(&dir)?;
    let mut to = dir.join(filename);
    let mut n = 1;
    while to.try_exists()? {
        to = dir.join(if n == 1 { format!("{}-{filename}", &hash[..8]) } else { format!("{}-{n}-{filename}", &hash[..8]) });
        n += 1;
    }
    std::fs::rename(store_dir(s).join(hash), to)
}

/// Applies renames and removals (quarantine/trash move the stored file to `<data>/quarantine/`); grouping,
/// alternates and skips are advice only.
async fn organize_apply(State(s): State<AppState>, admin: Admin) -> ApiResult<Json<Value>> {
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
        audit::record(&db, &admin.email, "organize_applied", "", &format!("{applied} changes applied"))?;
    }
    Ok(Json(json!({ "applied": applied })))
}
