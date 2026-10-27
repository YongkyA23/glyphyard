//! SQLite store and its embedded migrations (`PRAGMA user_version` counts applied steps).

use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;

const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE folders (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL,
    file_id TEXT,                 -- "<volume>:<id>" from the file-id crate
    status TEXT NOT NULL DEFAULT 'watching',   -- watching | offline
    offline_since TEXT,
    managed TEXT,                 -- 'shared' | 'google' for folders the app owns
    added_at TEXT NOT NULL
);
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    folder_id INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    rel_path TEXT NOT NULL,
    hash TEXT NOT NULL,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    format TEXT NOT NULL,
    added_at TEXT NOT NULL,
    UNIQUE (folder_id, rel_path)
);
CREATE INDEX files_hash ON files(hash);
CREATE TABLE faces (
    hash TEXT NOT NULL,
    idx INTEGER NOT NULL,
    family_key TEXT NOT NULL,
    ps_name TEXT NOT NULL,
    family TEXT NOT NULL,
    style TEXT NOT NULL,
    full_name TEXT NOT NULL,
    version TEXT NOT NULL,
    foundry TEXT NOT NULL,
    designer TEXT NOT NULL,
    weight REAL NOT NULL,
    italic INTEGER NOT NULL,
    is_variable INTEGER NOT NULL,
    axes_json TEXT NOT NULL,
    features_json TEXT NOT NULL,
    glyph_count INTEGER NOT NULL,
    PRIMARY KEY (hash, idx)
);
CREATE INDEX faces_ps ON faces(ps_name COLLATE NOCASE);
CREATE INDEX faces_family ON faces(family_key);
CREATE TABLE display_overrides (hash TEXT PRIMARY KEY, display_name TEXT NOT NULL);
-- The ledger: one row per active file.
CREATE TABLE activations (
    hash TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    full_name TEXT NOT NULL,
    source TEXT NOT NULL,
    activated_at TEXT NOT NULL
);
CREATE TABLE blocked (ps_name TEXT PRIMARY KEY COLLATE NOCASE, hash TEXT);
CREATE TABLE collections (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    parent_id INTEGER REFERENCES collections(id) ON DELETE CASCADE,
    builtin TEXT UNIQUE
);
CREATE TABLE collection_items (
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    family_key TEXT NOT NULL,
    added_at TEXT NOT NULL,
    PRIMARY KEY (collection_id, family_key)
);
INSERT INTO collections (name, builtin) VALUES ('Favorites', 'favorites'), ('Recent', 'recent');
CREATE TABLE family_tags (family_key TEXT NOT NULL, tag TEXT NOT NULL, PRIMARY KEY (family_key, tag));
CREATE TABLE licenses (family_key TEXT PRIMARY KEY, label TEXT NOT NULL, file_path TEXT, note TEXT NOT NULL DEFAULT '');
CREATE TABLE project_sets (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    is_on INTEGER NOT NULL DEFAULT 0,
    last_used TEXT
);
CREATE TABLE project_set_items (
    set_id INTEGER NOT NULL REFERENCES project_sets(id) ON DELETE CASCADE,
    family_key TEXT NOT NULL,
    PRIMARY KEY (set_id, family_key)
);
CREATE TABLE history (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    at TEXT NOT NULL,
    undone INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#,
    // 2: folders whose new fonts are turned on as they arrive.
    "CREATE TABLE auto_folders (path TEXT PRIMARY KEY);",
];

pub(crate) fn open(path: &Path) -> rusqlite::Result<Connection> {
    let mut conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let applied: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(applied as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
        tx.commit()?;
    }
    Ok(())
}

pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub(crate) fn get<T: DeserializeOwned>(conn: &Connection, key: &str) -> rusqlite::Result<Option<T>> {
    let raw: Option<String> =
        conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?;
    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

pub(crate) fn set<T: Serialize + ?Sized>(conn: &Connection, key: &str, value: &T) -> rusqlite::Result<()> {
    let json = serde_json::to_string(value).expect("settings serialize");
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, json],
    )?;
    Ok(())
}

pub(crate) fn remove(conn: &Connection, key: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    Ok(())
}
