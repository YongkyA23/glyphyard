//! Admin views of installs: overview, list, detail, timeline, remote control, name history, merge/split.

use crate::auth::{self, Admin};
use crate::{audit, db, db::GROUP, library, hub, hub::Hub, ApiError, ApiResult, AppState, Page, Paging};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use typeshelf_core::name_rule::next_allowed;
use typeshelf_protocol::{CommandKind, FontRef};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/overview", get(overview))
        .route("/installs", get(list))
        .route("/installs/merge", post(merge))
        .route("/installs/{id}", get(detail).delete(revoke))
        .route("/installs/{id}/timeline", get(timeline))
        .route("/installs/{id}/fonts", get(fonts).post(set_font))
        .route("/installs/{id}/families", get(families))
        .route("/installs/{id}/fonts/bulk", post(bulk_fonts))
        .route("/installs/{id}/reset-name-limit", post(reset_name_limit))
        .route("/installs/{id}/organize-request", post(organize_request))
        .route("/installs/{id}/split", post(split))
        .route("/installs/{id}/computers/{computer}", delete(remove_computer))
        .route("/name-changes", get(name_changes))
        .route("/name-changes.csv", get(name_changes_csv))
        .route("/organize/reports", get(organize_reports))
}

const ONLINE_WINDOW_MIN: i64 = 5;
const STALE_DAYS: i64 = 14;
const NEW_DAYS: i64 = 7;
const SPLIT_DAYS: i64 = 30;

#[derive(Debug, Serialize)]
pub struct InstallSummary {
    id: String,
    name: String,
    previous_names: Vec<String>,
    os: String,
    app_version: String,
    active_fonts: i64,
    last_seen: String,
    online: bool,
    next_name_change: Option<DateTime<Utc>>,
    flags: Vec<&'static str>,
    #[serde(skip)]
    first_seen: String,
}

fn ago(d: Duration) -> String {
    db::ts(Utc::now() - d)
}

/// A known, not revoked install (revoked ones answer like unknown ones).
pub(crate) fn exists(c: &Connection, id: &str) -> ApiResult<()> {
    let found: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM installs WHERE id = ?1 AND revoked_at IS NULL)", [id], |r| r.get(0))?;
    found.then_some(()).ok_or_else(|| ApiError::not_found(format!("no install {id}")))
}

fn is_online(hub: &Hub, id: &str, last_seen: &str) -> bool {
    hub.is_connected(id) || last_seen >= ago(Duration::minutes(ONLINE_WINDOW_MIN)).as_str()
}

/// Everything except the `possible_duplicate` flag, which needs all installs.
fn summary(c: &Connection, hub: &Hub, days: u32, id: &str) -> rusqlite::Result<InstallSummary> {
    let (name, os, app_version, first_seen, last_seen): (String, String, String, String, String) = c.query_row(
        "SELECT name, os, app_version, first_seen, last_seen FROM installs WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let mut stmt = c.prepare(&format!("SELECT new_name FROM name_changes WHERE install_id IN {GROUP} ORDER BY at DESC"))?;
    let mut previous_names: Vec<String> = Vec::new();
    for n in stmt.query_map([id], |r| r.get::<_, String>(0))? {
        let n = n?;
        if n != name && !previous_names.contains(&n) {
            previous_names.push(n);
        }
    }
    let active_fonts = c.query_row("SELECT COUNT(*) FROM font_states WHERE install_id = ?1 AND active = 1", [id], |r| r.get(0))?;
    let has_override: bool =
        c.query_row("SELECT EXISTS(SELECT 1 FROM font_states WHERE install_id = ?1 AND overridden_at IS NOT NULL)", [id], |r| r.get(0))?;
    let has_failures: bool = c.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM events WHERE install_id IN {GROUP} AND kind = 'failure' AND at >= ?2)"),
        params![id, ago(Duration::days(7))],
        |r| r.get(0),
    )?;
    let online = is_online(hub, id, &last_seen);
    let mut flags = Vec::new();
    if first_seen >= ago(Duration::days(NEW_DAYS)) {
        flags.push("new");
    }
    if has_override {
        flags.push("override");
    }
    if has_failures {
        flags.push("failures");
    }
    if last_seen < ago(Duration::days(STALE_DAYS)) {
        flags.push("stale");
    }
    Ok(InstallSummary {
        id: id.to_string(),
        next_name_change: db::next_name_change(c, id, days)?,
        name,
        previous_names,
        os,
        app_version,
        active_fonts,
        last_seen,
        online,
        flags,
        first_seen,
    })
}

/// Lowercase letters and digits only, so "Budi S." and "budi s" compare equal.
fn name_key(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn similar(a: &str, b: &str) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    // ponytail: ignore 1–2 letter names, or every "B" would match every "Budi".
    short == long || (short.chars().count() >= 3 && long.starts_with(short))
}

/// All installs not merged away, with flags.
fn summaries(c: &Connection, hub: &Hub) -> rusqlite::Result<Vec<InstallSummary>> {
    let days = db::settings(c)?.name_change_days;
    let ids = db::active_install_ids(c)?;
    let mut list = ids.iter().map(|id| summary(c, hub, days, id)).collect::<rusqlite::Result<Vec<_>>>()?;
    list.sort_by_key(|s| s.name.to_lowercase());
    let keys: Vec<Vec<String>> =
        list.iter().map(|s| std::iter::once(&s.name).chain(&s.previous_names).map(|n| name_key(n)).collect()).collect();
    // ponytail: O(n²) name comparison; fine for hundreds of installs.
    for i in 0..list.len() {
        let dup = (0..list.len()).any(|j| j != i && keys[i].iter().any(|a| keys[j].iter().any(|b| similar(a, b))));
        if dup {
            let after_new = usize::from(list[i].flags.first() == Some(&"new"));
            list[i].flags.insert(after_new, "possible_duplicate");
        }
    }
    Ok(list)
}

#[derive(Deserialize)]
struct ListQuery {
    q: Option<String>,
    filter: Option<String>,
    sort: Option<String>,
    dir: Option<String>,
}

async fn list(
    State(s): State<AppState>,
    _: Admin,
    Query(q): Query<ListQuery>,
    Query(paging): Query<Paging>,
) -> ApiResult<Json<Page<InstallSummary>>> {
    let flag = match q.filter.as_deref().unwrap_or("all") {
        "all" | "online" => None,
        "overrides" => Some("override"),
        "duplicates" => Some("possible_duplicate"),
        "stale" => Some("stale"),
        other => return Err(ApiError::bad_request("invalid_filter", format!("unknown filter `{other}`"))),
    };
    let sort = crate::sort_dir(q.sort.as_deref(), q.dir.as_deref(), &[("name", false), ("last_seen", true), ("fonts", true)])?;
    let needle = q.q.unwrap_or_default().trim().to_lowercase();
    let mut list: Vec<InstallSummary> = summaries(&s.db(), &s.hub)?
        .into_iter()
        .filter(|i| q.filter.as_deref() != Some("online") || i.online)
        .filter(|i| flag.is_none_or(|f| i.flags.contains(&f)))
        .filter(|i| {
            needle.is_empty()
                || i.id.to_lowercase().contains(&needle)
                || std::iter::once(&i.name).chain(&i.previous_names).any(|n| n.to_lowercase().contains(&needle))
        })
        .collect();
    if let Some((key, desc)) = sort {
        list.sort_by(|a, b| {
            let by = match key.as_str() {
                "last_seen" => a.last_seen.cmp(&b.last_seen),
                "fonts" => a.active_fonts.cmp(&b.active_fonts),
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            };
            let by = if desc { by.reverse() } else { by };
            by.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
    }
    // ponytail: builds every summary then slices, since `online` and `possible_duplicate` are computed in Rust; fine for hundreds of installs.
    Ok(Json(paging.slice(list)))
}

#[derive(Serialize)]
struct FontState {
    hash: String,
    ps_name: String,
    family: String,
    style: String,
    active: bool,
    source: String,
    overridden: bool,
    updated_at: String,
    in_library: bool,
    blocked: bool,
}

const FONT_COLUMNS: &str = "hash, ps_name, family, style, active, source, overridden, updated_at, in_library, blocked";

fn font_row(r: &rusqlite::Row) -> rusqlite::Result<FontState> {
    Ok(FontState {
        hash: r.get(0)?,
        ps_name: r.get(1)?,
        family: r.get(2)?,
        style: r.get(3)?,
        active: r.get(4)?,
        source: r.get(5)?,
        overridden: r.get(6)?,
        updated_at: r.get(7)?,
        in_library: r.get(8)?,
        blocked: r.get(9)?,
    })
}

const SOURCES: [&str; 4] = ["user", "admin", "library", "project_set"];

/// The filters shared by the font list, the family list and bulk actions.
#[derive(Deserialize, Default)]
struct FontFilter {
    q: Option<String>,
    active: Option<bool>,
    source: Option<String>,
    overridden: Option<bool>,
    family: Option<String>,
}

/// One filtered view of an install's fonts: the install and every install merged into it, one row per hash (on if
/// any has it on, the other fields from the newest report). `fonts`, `families` and `bulk_fonts` all read it, so
/// they can't disagree. Binds `?1`..`?6`; callers add their own from `?7`.
struct Scope<'a> {
    id: &'a str,
    filter: &'a FontFilter,
    needle: String,
}

impl<'a> Scope<'a> {
    const SQL: &'static str = "WITH g AS (
        SELECT s.hash, s.ps_name, s.family, s.style, s.source, s.overridden_at IS NOT NULL AS overridden, s.updated_at,
               s.any_active AS active, l.hash IS NOT NULL AS in_library, COALESCE(l.blocked, 0) AS blocked
        FROM (SELECT *, MAX(active) OVER (PARTITION BY hash) AS any_active,
                     ROW_NUMBER() OVER (PARTITION BY hash ORDER BY updated_at DESC, install_id) AS newest
              FROM font_states WHERE install_id IN (SELECT id FROM installs WHERE id = ?1 OR merged_into = ?1)) s
        LEFT JOIN library_files l ON l.hash = s.hash
        WHERE s.newest = 1
      ), f AS (
        SELECT * FROM g
        WHERE instr(lower(family || ' ' || style || ' ' || ps_name), ?2) > 0 AND (?3 IS NULL OR active = ?3)
          AND (?4 IS NULL OR source = ?4) AND (?5 IS NULL OR overridden = ?5) AND (?6 IS NULL OR family = ?6 COLLATE NOCASE)
      )";

    fn new(id: &'a str, filter: &'a FontFilter) -> ApiResult<Self> {
        if filter.source.as_deref().is_some_and(|s| !SOURCES.contains(&s)) {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid_source", "source must be user, admin, library or project_set"));
        }
        Ok(Self { id, filter, needle: filter.q.as_deref().unwrap_or_default().trim().to_lowercase() })
    }

    fn args(&self) -> Vec<&dyn rusqlite::ToSql> {
        let f = self.filter;
        vec![&self.id, &self.needle, &f.active, &f.source, &f.overridden, &f.family]
    }
}

/// Active first, then family; `?7`/`?8` are limit and offset.
async fn fonts(
    State(s): State<AppState>,
    _: Admin,
    Path(id): Path<String>,
    Query(q): Query<FontFilter>,
    Query(paging): Query<Paging>,
) -> ApiResult<Json<Page<FontState>>> {
    let db = s.db();
    exists(&db, &id)?;
    let (offset, limit) = paging.window(Paging::DEFAULT, Paging::MAX);
    let scope = Scope::new(&id, &q)?;
    let sql = Scope::SQL;
    let total: i64 = db.query_row(&format!("{sql} SELECT COUNT(*) FROM f"), scope.args().as_slice(), |r| r.get(0))?;
    let mut stmt = db.prepare(&format!(
        "{sql} SELECT {FONT_COLUMNS} FROM f ORDER BY active DESC, family COLLATE NOCASE, style, hash LIMIT ?7 OFFSET ?8"
    ))?;
    let mut args = scope.args();
    args.extend([&limit as &dyn rusqlite::ToSql, &offset]);
    let items = stmt.query_map(args.as_slice(), font_row)?.collect::<Result<_, _>>()?;
    Ok(Json(Page { items, total }))
}

#[derive(Deserialize)]
struct FamiliesQuery {
    sort: Option<String>,
}

#[derive(Serialize)]
struct FamilyRow {
    family: String,
    styles: i64,
    on: i64,
    sources: Vec<String>,
    overridden: i64,
    in_library: i64,
    updated_at: String,
}

/// Fonts grouped by family (case-insensitive) over the same scope as `fonts`; `total` counts families, `fonts` the
/// matching styles.
async fn families(
    State(s): State<AppState>,
    _: Admin,
    Path(id): Path<String>,
    Query(q): Query<FontFilter>,
    Query(fq): Query<FamiliesQuery>,
    Query(paging): Query<Paging>,
) -> ApiResult<Json<Value>> {
    let order = match fq.sort.as_deref() {
        None | Some("name") => "family COLLATE NOCASE",
        Some("recent") => "updated_at DESC, family COLLATE NOCASE",
        Some(_) => return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid_sort", "sort must be name or recent")),
    };
    let db = s.db();
    exists(&db, &id)?;
    let (offset, limit) = paging.window(Paging::DEFAULT, Paging::MAX);
    let scope = Scope::new(&id, &q)?;
    let sql = Scope::SQL;
    let (total, fonts): (i64, i64) = db.query_row(
        &format!("{sql} SELECT COUNT(DISTINCT family COLLATE NOCASE), COUNT(*) FROM f"),
        scope.args().as_slice(),
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut stmt = db.prepare(&format!(
        "{sql} SELECT MIN(family), COUNT(*), SUM(active), group_concat(DISTINCT source), SUM(overridden), SUM(in_library), MAX(updated_at) AS updated_at
         FROM f GROUP BY family COLLATE NOCASE ORDER BY {order} LIMIT ?7 OFFSET ?8"
    ))?;
    let mut args = scope.args();
    args.extend([&limit as &dyn rusqlite::ToSql, &offset]);
    let items: Vec<FamilyRow> = stmt
        .query_map(args.as_slice(), |r| {
            Ok(FamilyRow {
                family: r.get(0)?,
                styles: r.get(1)?,
                on: r.get(2)?,
                sources: r.get::<_, String>(3)?.split(',').map(str::to_string).collect(),
                overridden: r.get(4)?,
                in_library: r.get(5)?,
                updated_at: r.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(Json(json!({ "items": items, "total": total, "fonts": fonts })))
}

#[derive(Deserialize)]
struct BulkBody {
    action: String,
    families: Option<Vec<String>>,
    hashes: Option<Vec<String>>,
    all: Option<bool>,
    #[serde(flatten)]
    filter: FontFilter,
}

/// Turns fonts on, off or blocks them in one batch: the chosen families, hashes or everything, narrowed by the same
/// filters as the lists. `on` skips blocked library fonts and `block` skips fonts that are not in the library; both
/// count them in `skipped`. Fonts already in the wanted state are left alone and not counted.
async fn bulk_fonts(State(s): State<AppState>, admin: Admin, Path(id): Path<String>, Json(body): Json<BulkBody>) -> ApiResult<Json<Value>> {
    if !["on", "off", "block"].contains(&body.action.as_str()) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid_action", "action must be on, off or block"));
    }
    let all = body.all == Some(true);
    if usize::from(body.families.is_some()) + usize::from(body.hashes.is_some()) + usize::from(all) != 1 {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid_selection", "choose exactly one of families, hashes or all"));
    }
    let mut db = s.db();
    exists(&db, &id)?;
    let scope = Scope::new(&id, &body.filter)?;
    let (pick, list) = match (&body.families, &body.hashes) {
        (Some(f), _) => ("EXISTS (SELECT 1 FROM json_each(?7) WHERE value = family COLLATE NOCASE)", json!(f).to_string()),
        (_, Some(h)) => ("hash IN (SELECT value FROM json_each(?7))", json!(h).to_string()),
        _ => ("?7 IS NOT NULL", String::new()),
    };
    let sql = Scope::SQL;
    let mut args = scope.args();
    args.push(&list);
    let rows: Vec<FontState> = db
        .prepare(&format!("{sql} SELECT {FONT_COLUMNS} FROM f WHERE {pick} ORDER BY family COLLATE NOCASE, style, hash"))?
        .query_map(args.as_slice(), font_row)?
        .collect::<Result<_, _>>()?;

    // What the computer will be doing once its queue is worked off: the newest unacked on/off command per font
    // beats the reported state, so an undo before the ack reverses the queued change and a repeat adds nothing.
    let queued: HashMap<String, bool> = db
        .prepare(
            "SELECT json_extract(payload_json, '$.font.hash'), kind = 'activate' FROM commands
             WHERE install_id = ?1 AND acked_at IS NULL AND kind IN ('activate', 'deactivate') ORDER BY id",
        )?
        .query_map([&id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;

    let tx = db.transaction()?;
    let (mut changed, mut skipped) = (Vec::new(), 0);
    for f in rows {
        let active = queued.get(&f.hash).copied().unwrap_or(f.active);
        let wanted = match body.action.as_str() {
            "on" if f.blocked && !active => {
                skipped += 1;
                continue;
            }
            "on" => !active,
            "off" => active,
            _ if !f.in_library => {
                skipped += 1;
                continue;
            }
            _ => !f.blocked,
        };
        if !wanted {
            continue;
        }
        let font = FontRef { hash: f.hash.clone(), ps_name: f.ps_name, family: f.family, style: f.style };
        match body.action.as_str() {
            "on" => drop(hub::queue(&tx, &s.hub, &id, CommandKind::Activate { font })?),
            "off" => drop(hub::queue(&tx, &s.hub, &id, CommandKind::Deactivate { font })?),
            _ => drop(library::set_blocked(&tx, &s.hub, &f.hash, true)?),
        }
        changed.push(f.hash);
    }
    if !changed.is_empty() {
        audit::record(&tx, &admin.email, &format!("fonts_bulk_{}", body.action), &id, &format!("{} fonts", changed.len()))?;
    }
    tx.commit()?;
    Ok(Json(json!({ "changed": changed, "skipped": skipped })))
}

fn install_detail(c: &Connection, hub: &Hub, id: &str) -> ApiResult<Value> {
    exists(c, id)?;
    let summary = match summaries(c, hub)?.into_iter().find(|s| s.id == id) {
        Some(s) => s,
        None => summary(c, hub, db::settings(c)?.name_change_days, id)?, // merged away: not in the main list
    };

    let mut stmt = c.prepare("SELECT id FROM installs WHERE merged_into = ?1 AND revoked_at IS NULL ORDER BY first_seen")?;
    let merged_from: Vec<String> = stmt.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?;

    // The person's own install first (`db::person_devices` order).
    let mut computers = Vec::new();
    for device in db::person_devices(c, id)? {
        let (os, app_version, first_seen, last_seen, merged_at): (String, String, String, String, Option<String>) = c.query_row(
            "SELECT os, app_version, first_seen, last_seen, merged_at FROM installs WHERE id = ?1",
            [&device],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        let online = is_online(hub, &device, &last_seen);
        computers.push(json!({ "id": device, "os": os, "app_version": app_version, "first_seen": first_seen,
                               "last_seen": last_seen, "online": online, "merged_at": merged_at }));
    }

    let mut stmt = c.prepare(&format!("SELECT new_name, at FROM name_changes WHERE install_id IN {GROUP} ORDER BY at, id"))?;
    let names: Vec<(String, String)> = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    let name_history: Vec<Value> = names
        .iter()
        .enumerate()
        .map(|(i, (name, from))| json!({ "name": name, "from": from, "to": names.get(i + 1).map(|n| &n.1) }))
        .collect();

    let mut stmt = c.prepare(&format!(
        "SELECT {FONT_COLUMNS}, admin_at, overridden_at FROM (
           SELECT fs.*, fs.overridden_at IS NOT NULL AS overridden, l.hash IS NOT NULL AS in_library, COALESCE(l.blocked, 0) AS blocked
           FROM font_states fs LEFT JOIN library_files l ON l.hash = fs.hash)
         WHERE install_id = ?1 AND admin_at IS NOT NULL AND overridden_at IS NOT NULL ORDER BY family COLLATE NOCASE, style"
    ))?;
    let overrides: Vec<Value> = stmt
        .query_map([id], |r| Ok(json!({ "font": font_row(r)?, "admin_at": r.get::<_, String>(10)?, "user_at": r.get::<_, String>(11)? })))?
        .collect::<Result<_, _>>()?;

    let mut out = serde_json::to_value(&summary)?;
    let obj = out.as_object_mut().expect("object");
    obj.insert("first_seen".into(), json!(summary.first_seen));
    obj.insert("merged_from".into(), json!(merged_from));
    obj.insert("computers".into(), json!(computers));
    obj.insert("name_history".into(), json!(name_history));
    obj.insert("overrides".into(), json!(overrides));
    Ok(out)
}

async fn detail(State(s): State<AppState>, _: Admin, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    Ok(Json(install_detail(&s.db(), &s.hub, &id)?))
}

#[derive(Deserialize)]
struct TimelineQuery {
    kind: Option<String>,
}

async fn timeline(
    State(s): State<AppState>,
    _: Admin,
    Path(id): Path<String>,
    Query(q): Query<TimelineQuery>,
    Query(paging): Query<Paging>,
) -> ApiResult<Json<Page<Value>>> {
    let kind = match q.kind.as_deref().unwrap_or("all") {
        "all" => "",
        "names" => "name",
        "fonts" => "font",
        "overrides" => "override",
        "failures" => "failure",
        other => return Err(ApiError::bad_request("invalid_kind", format!("unknown timeline kind `{other}`"))),
    };
    let db = s.db();
    exists(&db, &id)?;
    let (offset, limit) = paging.window(100, 500);
    let filter = format!("install_id IN {GROUP} AND (?2 = '' OR kind = ?2)");
    let total: i64 = db.query_row(&format!("SELECT COUNT(*) FROM events WHERE {filter}"), params![id, kind], |r| r.get(0))?;
    let mut stmt = db.prepare(&format!(
        "SELECT id, at, by, kind, text FROM events WHERE {filter} ORDER BY at DESC, id DESC LIMIT ?3 OFFSET ?4"
    ))?;
    let items = stmt
        .query_map(params![id, kind, limit, offset], |r| {
            Ok(json!({ "id": r.get::<_, i64>(0)?, "at": r.get::<_, String>(1)?, "by": r.get::<_, String>(2)?,
                       "kind": r.get::<_, String>(3)?, "text": r.get::<_, String>(4)? }))
        })?
        .collect::<Result<_, _>>()?;
    Ok(Json(Page { items, total }))
}

#[derive(Deserialize)]
struct FontBody {
    hash: String,
    active: bool,
}

/// Queues Activate/Deactivate for the install and its merged installs. The font is looked up on that group first
/// (as the fonts list shows it), then in the shared library.
async fn set_font(State(s): State<AppState>, admin: Admin, Path(id): Path<String>, Json(body): Json<FontBody>) -> ApiResult<Json<Value>> {
    let db = s.db();
    exists(&db, &id)?;
    let font_ref = |r: &rusqlite::Row| Ok(FontRef { hash: body.hash.clone(), ps_name: r.get(0)?, family: r.get(1)?, style: r.get(2)? });
    let on_install = db
        .query_row(
            &format!("SELECT ps_name, family, style FROM font_states WHERE install_id IN {GROUP} AND hash = ?2 ORDER BY updated_at DESC LIMIT 1"),
            params![id, body.hash],
            font_ref,
        )
        .optional()?;
    let in_library: Option<(FontRef, bool)> = db
        .query_row("SELECT ps_name, family, style, blocked FROM library_files WHERE hash = ?1", [&body.hash], |r| {
            Ok((font_ref(r)?, r.get(3)?))
        })
        .optional()?;
    if body.active && in_library.as_ref().is_some_and(|(_, blocked)| *blocked) {
        return Err(ApiError::new(StatusCode::CONFLICT, "blocked", "this font is blocked in the shared library"));
    }
    let font = on_install
        .or(in_library.map(|(f, _)| f))
        .ok_or_else(|| ApiError::not_found("this install has never reported that font"))?;
    let detail = font.ps_name.clone();
    let kind = if body.active { CommandKind::Activate { font } } else { CommandKind::Deactivate { font } };
    let command_id = hub::queue(&db, &s.hub, &id, kind)?;
    audit::record(&db, &admin.email, if body.active { "font_on" } else { "font_off" }, &id, &detail)?;
    Ok(Json(json!({ "command_id": command_id })))
}

async fn reset_name_limit(State(s): State<AppState>, admin: Admin, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let db = s.db();
    exists(&db, &id)?;
    let now = db::now();
    db.execute("INSERT INTO name_limit_resets (install_id, at, admin_id) VALUES (?1, ?2, ?3)", params![id, now, admin.id])?;
    db::insert_event(&db, &id, "name", "admin", "Admin reset the name-change limit", "{}", &now)?;
    audit::record(&db, &admin.email, "name_limit_reset", &id, "")?;
    hub::notify_identity(&db, &s.hub, &id, None);
    Ok(StatusCode::NO_CONTENT)
}

async fn organize_request(State(s): State<AppState>, admin: Admin, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let db = s.db();
    exists(&db, &id)?;
    let command_id = hub::queue(&db, &s.hub, &id, CommandKind::OrganizeRequest)?;
    audit::record(&db, &admin.email, "organize_requested", &id, "")?;
    Ok(Json(json!({ "command_id": command_id })))
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum KeepName {
    Primary,
    Secondary,
}

#[derive(Deserialize)]
struct MergeBody {
    primary: String,
    secondary: String,
    keep_name: KeepName,
}

/// The older install always ends up primary; `keep_name` refers to the installs as named in the request.
async fn merge(State(s): State<AppState>, admin: Admin, Json(body): Json<MergeBody>) -> ApiResult<Json<Value>> {
    if body.primary == body.secondary {
        return Err(ApiError::bad_request("same_install", "pick two different installs"));
    }
    let mut db = s.db();
    let tx = db.transaction()?;
    let load = |id: &str| -> ApiResult<(String, String)> {
        let row: Option<(String, String, Option<String>)> = tx
            .query_row("SELECT name, first_seen, merged_into FROM installs WHERE id = ?1 AND revoked_at IS NULL", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        match row {
            None => Err(ApiError::not_found(format!("no install {id}"))),
            Some((_, _, Some(_))) => Err(ApiError::new(StatusCode::CONFLICT, "already_merged", format!("{id} is already merged"))),
            Some((name, first_seen, None)) => Ok((name, first_seen)),
        }
    };
    let (a_name, a_seen) = load(&body.primary)?;
    let (b_name, b_seen) = load(&body.secondary)?;
    let kept_name = match body.keep_name {
        KeepName::Primary => a_name.clone(),
        KeepName::Secondary => b_name.clone(),
    };
    let ((primary, primary_name), secondary) =
        if a_seen <= b_seen { ((body.primary, a_name), body.secondary) } else { ((body.secondary, b_name), body.primary) };
    let now = db::now();
    tx.execute("UPDATE installs SET merged_into = ?1, merged_at = ?2 WHERE id = ?3", params![primary, now, secondary])?;
    // Installs merged into the secondary follow it.
    tx.execute("UPDATE installs SET merged_into = ?1 WHERE merged_into = ?2", params![primary, secondary])?;
    if kept_name != primary_name {
        tx.execute("UPDATE installs SET name = ?1 WHERE id = ?2", params![kept_name, primary])?;
        tx.execute(
            "INSERT INTO name_changes (install_id, old_name, new_name, by, at) VALUES (?1, ?2, ?3, 'merge', ?4)",
            params![primary, primary_name, kept_name, now],
        )?;
    }
    db::insert_event(&tx, &primary, "system", "admin", &format!("Merged with install {secondary}"), "{}", &now)?;
    audit::record(&tx, &admin.email, "installs_merged", &primary, &format!("merged {secondary} into it, name {kept_name}"))?;
    tx.commit()?;
    hub::notify_identity(&db, &s.hub, &primary, None);
    Ok(Json(install_detail(&db, &s.hub, &primary)?))
}

/// Undoes a merge within 30 days; `id` is the retired install.
async fn split(State(s): State<AppState>, admin: Admin, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let mut db = s.db();
    let tx = db.transaction()?;
    exists(&tx, &id)?;
    let (primary, merged_at): (Option<String>, Option<String>) =
        tx.query_row("SELECT merged_into, merged_at FROM installs WHERE id = ?1", [&id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let (Some(primary), Some(merged_at)) = (primary, merged_at) else {
        return Err(ApiError::new(StatusCode::CONFLICT, "not_merged", "this install is not merged into another"));
    };
    if merged_at < ago(Duration::days(SPLIT_DAYS)) {
        return Err(ApiError::new(StatusCode::CONFLICT, "split_expired", "merges can only be undone within 30 days"));
    }
    // Undo the rename the merge made, if any.
    let renamed: Option<(i64, String)> = tx
        .query_row(
            "SELECT id, old_name FROM name_changes WHERE install_id = ?1 AND by = 'merge' AND at = ?2",
            params![primary, merged_at],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((change_id, old_name)) = renamed {
        tx.execute("UPDATE installs SET name = ?1 WHERE id = ?2", params![old_name, primary])?;
        tx.execute("DELETE FROM name_changes WHERE id = ?1", [change_id])?;
    }
    tx.execute("UPDATE installs SET merged_into = NULL, merged_at = NULL WHERE id = ?1", [&id])?;
    let now = db::now();
    db::insert_event(&tx, &primary, "system", "admin", &format!("Split install {id} back out"), "{}", &now)?;
    db::insert_event(&tx, &id, "system", "admin", &format!("Split back out of install {primary}"), "{}", &now)?;
    audit::record(&tx, &admin.email, "install_split", &id, &format!("out of {primary}"))?;
    tx.commit()?;
    hub::notify_identity(&db, &s.hub, &primary, None);
    hub::notify_identity(&db, &s.hub, &id, None);
    Ok(StatusCode::NO_CONTENT)
}

/// Removes a computer and every install merged into it (one person's computers): their tokens are refused, they
/// leave every list and count, deliveries aimed at them and unsent commands are dropped, and open sockets are
/// closed. Names and timeline events stay for the name-history export.
async fn revoke(State(s): State<AppState>, admin: Admin, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let ids = {
        let mut db = s.db();
        let tx = db.transaction()?;
        exists(&tx, &id)?;
        let mut stmt = tx.prepare(&format!("SELECT id FROM installs WHERE id IN {GROUP} AND revoked_at IS NULL"))?;
        let ids: Vec<String> = stmt.query_map([&id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        drop(stmt);
        let now = db::now();
        for i in &ids {
            revoke_one(&tx, i, &now)?;
        }
        audit::record(&tx, &admin.email, "install_revoked", &id, "")?;
        tx.commit()?;
        ids
    };
    for i in &ids {
        s.hub.kick(i);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// One computer's part of a removal: its token is refused, its fonts, unsent commands and deliveries go, and its
/// timeline says so. Runs in the caller's transaction; the caller kicks its socket after the commit.
fn revoke_one(c: &Connection, id: &str, now: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE installs SET revoked_at = ?1 WHERE id = ?2", params![now, id])?;
    c.execute("DELETE FROM font_states WHERE install_id = ?1", [id])?;
    // Cancelled, not deleted: a deleted command id could be reused (no AUTOINCREMENT).
    c.execute("UPDATE commands SET acked_at = ?1, ok = 0, error = 'cancelled' WHERE install_id = ?2 AND acked_at IS NULL", params![now, id])?;
    c.execute("DELETE FROM deliveries WHERE target = ?1", [id])?;
    db::insert_event(c, id, "system", "admin", "Removed by an admin", "{}", now)
}

/// Removes one of a person's computers and returns the id the person is listed under now. A merged computer is
/// revoked on its own. The person's own install hands the person to its earliest-registered remaining computer,
/// which takes the person's name and deliveries; the removed install stays merged into it, so its history still
/// belongs to the person. The only computer goes like the whole person.
async fn remove_computer(State(s): State<AppState>, admin: Admin, Path((id, computer)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let person = {
        let mut db = s.db();
        let tx = db.transaction()?;
        exists(&tx, &id)?;
        exists(&tx, &computer)?;
        let devices = db::person_devices(&tx, &id)?;
        if db::person(&tx, &id)? != id || !devices.contains(&computer) {
            return Err(ApiError::not_found(format!("{computer} is not a computer of {id}")));
        }
        let now = db::now();
        let mut person = id.clone();
        match devices.get(1) {
            Some(next) if computer == id => {
                let name = db::install_name(&tx, &id)?.unwrap_or_default();
                // Its own row's name; `install_name` would read the person's while it is still merged.
                let own: String = tx.query_row("SELECT name FROM installs WHERE id = ?1", [next], |r| r.get(0))?;
                // Everything merged into the person follows, removed computers too, so their history moves along.
                tx.execute("UPDATE installs SET merged_into = ?1 WHERE merged_into = ?2", params![next, id])?;
                tx.execute("UPDATE installs SET merged_into = NULL, merged_at = NULL WHERE id = ?1", [next])?;
                if own != name {
                    tx.execute("UPDATE installs SET name = ?1 WHERE id = ?2", params![name, next])?;
                    tx.execute(
                        "INSERT INTO name_changes (install_id, old_name, new_name, by, at) VALUES (?1, ?2, ?3, 'merge', ?4)",
                        params![next, own, name, now],
                    )?;
                }
                // Deliveries to the person keep reaching its computers; duplicates are dropped by `revoke_one`.
                tx.execute("UPDATE OR IGNORE deliveries SET target = ?1 WHERE target = ?2", params![next, id])?;
                revoke_one(&tx, &id, &now)?;
                tx.execute("UPDATE installs SET merged_into = ?1 WHERE id = ?2", params![next, id])?;
                person = next.clone();
            }
            _ => revoke_one(&tx, &computer, &now)?,
        }
        audit::record(&tx, &admin.email, "computer_removed", &id, &computer)?;
        tx.commit()?;
        person
    };
    s.hub.kick(&computer);
    Ok(Json(json!({ "person": person })))
}

/// Moves a revoked install's rows to a new id (`<id>~revoked-<hex>`) so the computer can register again under its
/// old id as a new install. Run inside the caller's transaction.
pub fn free_revoked_id(c: &Connection, id: &str) -> rusqlite::Result<()> {
    let moved = format!("{id}~revoked-{}", auth::random_hex(4));
    // The children are re-pointed before their parent row; the check runs at commit.
    c.pragma_update(None, "defer_foreign_keys", true)?;
    for table in ["name_changes", "name_limit_resets", "font_states", "events", "commands", "font_requests"] {
        c.execute(&format!("UPDATE {table} SET install_id = ?1 WHERE install_id = ?2"), [&moved, id])?;
    }
    c.execute("UPDATE installs SET merged_into = ?1 WHERE merged_into = ?2", [&moved, id])?;
    c.execute("UPDATE installs SET id = ?1 WHERE id = ?2", [&moved, id])?;
    Ok(())
}

#[derive(Serialize)]
struct NameChangeRow {
    at: String,
    install_ids: Vec<String>,
    old_name: Option<String>,
    new_name: String,
    by: String,
    next_allowed: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct NameQuery {
    q: Option<String>,
    since: Option<String>,
}

fn parse_since(s: &str) -> ApiResult<String> {
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Ok(db::ts(d.with_timezone(&Utc)));
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|d| db::ts(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).expect("midnight"))))
        .map_err(|_| ApiError::bad_request("invalid_since", "since must be a date (YYYY-MM-DD) or an RFC 3339 timestamp"))
}

/// `install_ids` = the install the change belongs to now (the merge primary) followed by the installs merged into it.
fn name_change_rows(c: &Connection, q: &NameQuery) -> ApiResult<Vec<NameChangeRow>> {
    let since = q.since.as_deref().filter(|s| !s.is_empty()).map(parse_since).transpose()?.unwrap_or_default();
    let needle = q.q.as_deref().unwrap_or_default().trim().to_lowercase();
    let days = db::settings(c)?.name_change_days;
    let mut stmt = c.prepare(
        "SELECT nc.at, COALESCE(i.merged_into, i.id), nc.old_name, nc.new_name, nc.by
           FROM name_changes nc JOIN installs i ON i.id = nc.install_id WHERE nc.at >= ?1
         UNION ALL
         SELECT r.at, COALESCE(i.merged_into, i.id), NULL, i.name, 'admin_reset'
           FROM name_limit_resets r JOIN installs i ON i.id = r.install_id WHERE r.at >= ?1
         ORDER BY 1 DESC",
    )?;
    let raw: Vec<(String, String, Option<String>, String, String)> =
        stmt.query_map([since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<Result<_, _>>()?;
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    let mut rows = Vec::new();
    for (at, group, old_name, new_name, by) in raw {
        if !groups.contains_key(&group) {
            let mut stmt = c.prepare("SELECT id FROM installs WHERE merged_into = ?1 ORDER BY first_seen")?;
            let mut ids = vec![group.clone()];
            ids.extend(stmt.query_map([&group], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?);
            groups.insert(group.clone(), ids);
        }
        let install_ids = groups[&group].clone();
        let matches = needle.is_empty()
            || new_name.to_lowercase().contains(&needle)
            || old_name.as_deref().is_some_and(|n| n.to_lowercase().contains(&needle))
            || install_ids.iter().any(|i| i.to_lowercase().contains(&needle));
        if !matches {
            continue;
        }
        let next = if by == "user" {
            let reset: Option<String> = c.query_row(
                &format!("SELECT MIN(at) FROM name_limit_resets WHERE install_id IN {GROUP} AND at > ?2"),
                params![group, at],
                |r| r.get(0),
            )?;
            next_allowed(Utc::now(), Some(db::parse_ts(&at)), reset.as_deref().map(db::parse_ts), days)
        } else {
            None
        };
        rows.push(NameChangeRow { at, install_ids, old_name, new_name, by, next_allowed: next });
    }
    Ok(rows)
}

async fn name_changes(
    State(s): State<AppState>,
    _: Admin,
    Query(q): Query<NameQuery>,
    Query(paging): Query<Paging>,
) -> ApiResult<Json<Page<NameChangeRow>>> {
    // ponytail: builds every matching row (search spans merged install ids) then slices; move the search into SQL if history reaches tens of thousands.
    Ok(Json(paging.slice(name_change_rows(&s.db(), &q)?)))
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

async fn name_changes_csv(State(s): State<AppState>, _: Admin, Query(q): Query<NameQuery>) -> ApiResult<impl IntoResponse> {
    let rows = name_change_rows(&s.db(), &q)?;
    let mut out = String::from("at,install_ids,old_name,new_name,by,next_allowed\r\n");
    for r in rows {
        let fields = [
            r.at,
            r.install_ids.join(";"),
            r.old_name.unwrap_or_default(),
            r.new_name,
            r.by,
            r.next_allowed.map(db::ts).unwrap_or_default(),
        ];
        out.push_str(&fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    Ok((
        [(header::CONTENT_TYPE, "text/csv; charset=utf-8"), (header::CONTENT_DISPOSITION, "attachment; filename=\"name-changes.csv\"")],
        out,
    ))
}

async fn organize_reports(State(s): State<AppState>, _: Admin, Query(paging): Query<Paging>) -> ApiResult<Json<Page<Value>>> {
    let db = s.db();
    let mut ids = db::active_install_ids(&db)?;
    ids.sort_by_cached_key(|id| db::install_name(&db, id).ok().flatten().unwrap_or_default().to_lowercase());
    let Page { items: ids, total } = paging.slice(ids);
    let mut out = Vec::new();
    for id in ids {
        let name = db::install_name(&db, &id)?.unwrap_or_default();
        let report: Option<String> = db
            .query_row(
                &format!(
                    "SELECT payload_json FROM events WHERE install_id IN {GROUP} AND kind = 'organize'
                     AND json_extract(payload_json, '$.kind') = 'duplicates_report' ORDER BY at DESC, id DESC LIMIT 1"
                ),
                [&id],
                |r| r.get(0),
            )
            .optional()?;
        let report: Value = report.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default();
        let last_organized: Option<String> = db.query_row(
            &format!(
                "SELECT MAX(at) FROM events WHERE install_id IN {GROUP} AND json_extract(payload_json, '$.kind') = 'organize_applied'"
            ),
            [&id],
            |r| r.get(0),
        )?;
        let n = |k: &str| report.get(k).and_then(Value::as_u64).unwrap_or(0);
        out.push(json!({
            "install_id": id, "name": name,
            "duplicates": n("duplicates"), "version_conflicts": n("version_conflicts"), "name_clashes": n("name_clashes"),
            "last_organized": last_organized,
        }));
    }
    Ok(Json(Page { items: out, total }))
}

async fn overview(State(s): State<AppState>, _: Admin) -> ApiResult<Json<Value>> {
    let db = s.db();
    let list = summaries(&db, &s.hub)?;
    let count_flag = |f: &str| list.iter().filter(|i| i.flags.contains(&f)).count();
    let os_count = |needles: &[&str]| list.iter().filter(|i| needles.iter().any(|n| i.os.to_lowercase().contains(n))).count();
    let today = Utc::now().date_naive();
    let month_start = db::ts(Utc.from_utc_datetime(&today.with_day(1).expect("day 1").and_hms_opt(0, 0, 0).expect("midnight")));
    let week = ago(Duration::days(7));
    let count = |sql: &str, since: &str| -> rusqlite::Result<i64> { db.query_row(sql, [since], |r| r.get(0)) };
    // Rows `x` of computers whose person (the merge primary) is not removed.
    let live = "x JOIN installs i ON i.id = x.install_id JOIN installs p ON p.id = COALESCE(i.merged_into, i.id) WHERE p.revoked_at IS NULL AND x.at >= ?1";
    let name_changes_month = count(&format!("SELECT COUNT(*) FROM name_changes {live} AND x.by = 'user'"), &month_start)?;
    let overrides_week = count(&format!("SELECT COUNT(*) FROM events {live} AND x.kind = 'override'"), &week)?;
    let failures_week = count(&format!("SELECT COUNT(*) FROM events {live} AND x.kind = 'failure'"), &week)?;

    let mut stmt = db.prepare(
        "SELECT e.id, p.id, p.name, e.kind, e.text, e.by, e.at FROM events e
         JOIN installs i ON i.id = e.install_id JOIN installs p ON p.id = COALESCE(i.merged_into, i.id)
         WHERE p.revoked_at IS NULL ORDER BY e.at DESC, e.id DESC LIMIT 20",
    )?;
    let recent: Vec<Value> = stmt
        .query_map([], |r| {
            Ok(json!({ "id": r.get::<_, i64>(0)?, "install_id": r.get::<_, String>(1)?, "install_name": r.get::<_, String>(2)?,
                       "kind": r.get::<_, String>(3)?, "text": r.get::<_, String>(4)?, "by": r.get::<_, String>(5)?,
                       "at": r.get::<_, String>(6)? }))
        })?
        .collect::<Result<_, _>>()?;

    let mut stmt = db.prepare(
        "SELECT f.family, COUNT(DISTINCT f.install_id) FROM font_states f JOIN installs i ON i.id = f.install_id
         WHERE f.active = 1 AND i.merged_into IS NULL GROUP BY f.family ORDER BY 2 DESC, 1 LIMIT 10",
    )?;
    let top_fonts: Vec<Value> = stmt
        .query_map([], |r| Ok(json!({ "family": r.get::<_, String>(0)?, "users": r.get::<_, i64>(1)? })))?
        .collect::<Result<_, _>>()?;

    // Last 30 UTC days, oldest first, zero-filled.
    let first = today - Duration::days(29);
    let since = db::ts(Utc.from_utc_datetime(&first.and_hms_opt(0, 0, 0).expect("midnight")));
    let mut days: HashMap<String, (i64, i64, i64)> = HashMap::new();
    let mut stmt = db.prepare(&format!(
        "SELECT substr(x.at, 1, 10), COUNT(*), COALESCE(SUM(x.kind = 'failure'), 0) FROM events {live} GROUP BY 1"
    ))?;
    for row in stmt.query_map([&since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))? {
        let (day, activity, failures) = row?;
        let e = days.entry(day).or_default();
        (e.0, e.1) = (activity, failures);
    }
    let mut stmt = db.prepare(
        "SELECT substr(first_seen, 1, 10), COUNT(*) FROM installs
         WHERE merged_into IS NULL AND revoked_at IS NULL AND first_seen >= ?1 GROUP BY 1",
    )?;
    for row in stmt.query_map([&since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (day, n) = row?;
        days.entry(day).or_default().2 = n;
    }
    let trend: Vec<Value> = (0..30)
        .map(|i| {
            let day = (first + Duration::days(i)).to_string();
            let (activity, failures, new_installs) = days.get(&day).copied().unwrap_or_default();
            json!({ "day": day, "activity": activity, "failures": failures, "new_installs": new_installs })
        })
        .collect();

    Ok(Json(json!({
        "trend": trend,
        "installs": list.len(),
        "online": list.iter().filter(|i| i.online).count(),
        "os": { "windows": os_count(&["win"]), "macos": os_count(&["mac", "darwin"]) },
        "name_changes_month": name_changes_month,
        "overrides_week": overrides_week,
        "failures_week": failures_week,
        "recent": recent,
        "attention": {
            "overrides": count_flag("override"),
            "failures": count_flag("failures"),
            "possible_duplicates": count_flag("possible_duplicate"),
            "stale": count_flag("stale"),
            "requests": crate::requests::open_count(&db)?,
        },
        "top_fonts": top_fonts,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_names() {
        assert!(similar(&name_key("Budi S."), &name_key("Budi Santoso")));
        assert!(similar(&name_key("budi  santoso"), &name_key("Budi Santoso")));
        assert!(!similar(&name_key("Ana"), &name_key("Budi")));
        assert!(!similar(&name_key("B"), &name_key("Budi")));
    }

    #[test]
    fn csv_quotes() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("plain"), "plain");
    }
}
