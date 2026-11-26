//! SQLite connection, embedded migrations, timestamps, settings and shared queries.

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::Path;
use typeshelf_protocol::Policy;

/// Applied in order; `PRAGMA user_version` records how many have run.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_command_fanout.sql"),
    include_str!("../migrations/0003_font_requests.sql"),
    include_str!("../migrations/0004_revoke.sql"),
    include_str!("../migrations/0005_admin_roles_audit.sql"),
];

/// Opens (or creates) the database. `None` = in memory, for tests.
pub fn open(path: Option<&Path>) -> rusqlite::Result<Connection> {
    let mut conn = match path {
        Some(p) => Connection::open(p)?,
        None => Connection::open_in_memory()?,
    };
    conn.pragma_update(None, "foreign_keys", true)?;
    if path.is_some() {
        conn.pragma_update(None, "journal_mode", "WAL")?;
    }
    migrate(&mut conn)?;
    if settings(&conn)?.install_key.is_empty() {
        let mut s = settings(&conn)?;
        s.install_key = crate::auth::random_hex(16);
        save_settings(&conn, &s)?;
    }
    Ok(conn)
}

fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let done: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (version, sql) in (1..).zip(MIGRATIONS).skip(done as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

/// Fixed-width RFC 3339 so stored timestamps sort correctly as text.
pub fn ts(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Micros, true)
}

pub fn now() -> String {
    ts(Utc::now())
}

pub fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)).unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub name_change_days: u32,
    pub allow_connection_edit: bool,
    pub deliver_library_to_new: bool,
    pub activate_delivered: bool,
    pub notify_admin_changes: bool,
    pub timeline_retention_days: Option<u32>,
    pub server_url: String,
    pub install_key: String,
    /// Shown in every desktop app; empty = Typeshelf's own name and colour.
    pub brand_name: String,
    /// `#rrggbb` or empty.
    pub brand_color: String,
    /// `data:image/(png|jpeg|webp|svg+xml);base64,...` or empty.
    // ponytail: kept inline in settings and in every policy push; a served file plus a hash if logos ever grow past the cap.
    pub brand_logo: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            name_change_days: 30,
            allow_connection_edit: true,
            deliver_library_to_new: true,
            activate_delivered: false,
            notify_admin_changes: true,
            timeline_retention_days: None,
            server_url: String::new(),
            install_key: String::new(),
            brand_name: String::new(),
            brand_color: String::new(),
            brand_logo: String::new(),
        }
    }
}

impl Settings {
    pub fn policy(&self) -> Policy {
        Policy {
            name_change_days: self.name_change_days,
            allow_connection_edit: self.allow_connection_edit,
            notify_admin_changes: self.notify_admin_changes,
            brand_name: self.brand_name.clone(),
            brand_color: self.brand_color.clone(),
            brand_logo: self.brand_logo.clone(),
        }
    }
}

pub fn settings(c: &Connection) -> rusqlite::Result<Settings> {
    let mut stmt = c.prepare("SELECT key, value FROM settings")?;
    let map = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(|row| row.ok().and_then(|(k, v)| Some((k, serde_json::from_str(&v).ok()?))))
        .collect::<Map<String, Value>>();
    Ok(serde_json::from_value(Value::Object(map)).unwrap_or_default())
}

pub fn save_settings(c: &Connection, s: &Settings) -> rusqlite::Result<()> {
    let Ok(Value::Object(map)) = serde_json::to_value(s) else { unreachable!("settings serialize to an object") };
    for (k, v) in map {
        c.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![k, v.to_string()],
        )?;
    }
    Ok(())
}

/// SQL for "this install and every install merged into it"; bind the install id as `?1`.
pub const GROUP: &str = "(SELECT id FROM installs WHERE id = ?1 OR merged_into = ?1)";

/// When this install (with everything merged into it) may change its name next; `None` = now.
pub fn next_name_change(c: &Connection, id: &str, days: u32) -> rusqlite::Result<Option<DateTime<Utc>>> {
    let last_change: Option<String> = c.query_row(
        &format!("SELECT MAX(at) FROM name_changes WHERE by = 'user' AND install_id IN {GROUP}"),
        [id],
        |r| r.get(0),
    )?;
    let last_reset: Option<String> =
        c.query_row(&format!("SELECT MAX(at) FROM name_limit_resets WHERE install_id IN {GROUP}"), [id], |r| r.get(0))?;
    Ok(typeshelf_core::name_rule::next_allowed(
        Utc::now(),
        last_change.as_deref().map(parse_ts),
        last_reset.as_deref().map(parse_ts),
        days,
    ))
}

pub fn insert_event(c: &Connection, install_id: &str, kind: &str, by: &str, text: &str, payload: &str, at: &str) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO events (install_id, kind, by, text, payload_json, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![install_id, kind, by, text, payload, at],
    )?;
    Ok(())
}

pub fn install_name(c: &Connection, id: &str) -> rusqlite::Result<Option<String>> {
    c.query_row("SELECT name FROM installs WHERE id = ?1", [id], |r| r.get(0)).optional()
}

/// Every install not merged away or revoked: the admin's list, and the targets of broadcasts (block, policy,
/// deliver to all), which `hub::queue` fans out to the installs merged into each.
pub fn active_install_ids(c: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = c.prepare("SELECT id FROM installs WHERE merged_into IS NULL AND revoked_at IS NULL")?;
    let ids = stmt.query_map([], |r| r.get(0))?.collect();
    ids
}
