//! `.typeshelf` backups: a zip holding one JSON export of collections, tags, licenses, project sets
//! and app settings. No font files, no identity or connection secrets.

use crate::{store, AppSettings, BackupInfo, Engine, EngineError, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;

const ENTRY: &str = "typeshelf.json";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Export {
    version: u32,
    collections: Vec<ExportCollection>,
    tags: Vec<(String, String)>,
    licenses: Vec<ExportLicense>,
    project_sets: Vec<ExportSet>,
    settings: Option<AppSettings>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportCollection {
    id: i64,
    name: String,
    parent_id: Option<i64>,
    builtin: Option<String>,
    items: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportLicense {
    family_key: String,
    label: String,
    file: Option<String>,
    note: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportSet {
    id: i64,
    name: String,
    note: String,
    family_keys: Vec<String>,
}

fn strings(conn: &Connection, sql: &str, id: i64) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map([id], |r| r.get(0))?;
    rows.collect()
}

fn export(conn: &Connection) -> rusqlite::Result<Export> {
    let mut stmt = conn.prepare("SELECT id, name, parent_id, builtin FROM collections ORDER BY id")?;
    let mut collections: Vec<ExportCollection> = stmt
        .query_map([], |r| {
            Ok(ExportCollection { id: r.get(0)?, name: r.get(1)?, parent_id: r.get(2)?, builtin: r.get(3)?, items: vec![] })
        })?
        .collect::<rusqlite::Result<_>>()?;
    for c in &mut collections {
        c.items = strings(conn, "SELECT family_key FROM collection_items WHERE collection_id = ?1 ORDER BY added_at", c.id)?;
    }
    let mut stmt = conn.prepare("SELECT family_key, tag FROM family_tags ORDER BY family_key, tag")?;
    let tags = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    let mut stmt = conn.prepare("SELECT family_key, label, file_path, note FROM licenses ORDER BY family_key")?;
    let licenses = stmt
        .query_map([], |r| Ok(ExportLicense { family_key: r.get(0)?, label: r.get(1)?, file: r.get(2)?, note: r.get(3)? }))?
        .collect::<rusqlite::Result<_>>()?;
    let mut stmt = conn.prepare("SELECT id, name, note FROM project_sets ORDER BY id")?;
    let mut project_sets: Vec<ExportSet> = stmt
        .query_map([], |r| Ok(ExportSet { id: r.get(0)?, name: r.get(1)?, note: r.get(2)?, family_keys: vec![] }))?
        .collect::<rusqlite::Result<_>>()?;
    for s in &mut project_sets {
        s.family_keys = strings(conn, "SELECT family_key FROM project_set_items WHERE set_id = ?1 ORDER BY family_key", s.id)?;
    }
    Ok(Export { version: 1, collections, tags, licenses, project_sets, settings: store::get(conn, "app")? })
}

impl Engine {
    pub fn backup_create(&self, path: impl AsRef<Path>) -> Result<BackupInfo> {
        let json = serde_json::to_vec_pretty(&export(&self.db())?)?;
        let file = std::fs::File::create(path.as_ref())?;
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(ENTRY, zip::write::SimpleFileOptions::default()).map_err(zip_err)?;
        zip.write_all(&json)?;
        zip.finish().map_err(zip_err)?;
        Ok(BackupInfo { path: path.as_ref().to_string_lossy().into_owned(), at: store::now() })
    }

    /// Replaces collections, tags, licenses, project sets and settings; undoable from history.
    pub fn backup_restore(&self, path: impl AsRef<Path>) -> Result<BackupInfo> {
        let file = std::fs::File::open(path.as_ref())?;
        let mut zip = zip::ZipArchive::new(file).map_err(zip_err)?;
        let mut json = String::new();
        zip.by_name(ENTRY).map_err(zip_err)?.read_to_string(&mut json)?;
        let incoming: Export =
            serde_json::from_str(&json).map_err(|e| EngineError::new("invalid_backup", e.to_string()))?;
        let previous = export(&self.db())?;
        self.import_backup(&incoming)?;
        let at = store::now();
        self.db().execute(
            "INSERT INTO history (kind, summary, payload_json, at) VALUES ('restore', ?1, ?2, ?3)",
            params![format!("Restored {}", path.as_ref().display()), serde_json::to_string(&previous)?, at],
        )?;
        self.library_changed("scan");
        Ok(BackupInfo { path: path.as_ref().to_string_lossy().into_owned(), at })
    }

    pub(crate) fn import_backup(&self, e: &Export) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        // Sets keep their on/off state when they survive the restore (fonts are not touched).
        let on: HashSet<i64> = {
            let mut stmt = tx.prepare("SELECT id FROM project_sets WHERE is_on = 1")?;
            let ids = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            ids
        };
        tx.execute_batch(
            "PRAGMA defer_foreign_keys = ON;
             DELETE FROM collection_items; DELETE FROM collections; DELETE FROM family_tags;
             DELETE FROM licenses; DELETE FROM project_set_items; DELETE FROM project_sets;",
        )?;
        for c in &e.collections {
            tx.execute(
                "INSERT INTO collections (id, name, parent_id, builtin) VALUES (?1, ?2, ?3, ?4)",
                params![c.id, c.name, c.parent_id, c.builtin],
            )?;
            for k in &c.items {
                tx.execute(
                    "INSERT OR IGNORE INTO collection_items (collection_id, family_key, added_at) VALUES (?1, ?2, ?3)",
                    params![c.id, k, store::now()],
                )?;
            }
        }
        tx.execute_batch(
            "INSERT OR IGNORE INTO collections (name, builtin) VALUES ('Favorites', 'favorites'), ('Recent', 'recent');",
        )?;
        for (k, t) in &e.tags {
            tx.execute("INSERT OR IGNORE INTO family_tags (family_key, tag) VALUES (?1, ?2)", params![k, t])?;
        }
        for l in &e.licenses {
            tx.execute(
                "INSERT OR REPLACE INTO licenses (family_key, label, file_path, note) VALUES (?1, ?2, ?3, ?4)",
                params![l.family_key, l.label, l.file, l.note],
            )?;
        }
        for s in &e.project_sets {
            tx.execute(
                "INSERT INTO project_sets (id, name, note, is_on) VALUES (?1, ?2, ?3, ?4)",
                params![s.id, s.name, s.note, on.contains(&s.id)],
            )?;
            for k in &s.family_keys {
                tx.execute("INSERT OR IGNORE INTO project_set_items (set_id, family_key) VALUES (?1, ?2)", params![s.id, k])?;
            }
        }
        if let Some(s) = &e.settings {
            store::set(&tx, "app", s)?;
        }
        tx.commit()?;
        Ok(())
    }
}

fn zip_err(e: zip::result::ZipError) -> EngineError {
    EngineError::new("invalid_backup", e.to_string())
}
