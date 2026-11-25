//! `.typeshelf` backups: a zip holding one JSON export of collections, tags, pins, licenses,
//! project sets and app settings. No font files, no identity or connection secrets. Also the
//! collections-only JSON of `export_collections` / `import_collections`.

use crate::{store, AppSettings, BackupInfo, CollectionsExport, CollectionsImport, Engine, EngineError, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;

const ENTRY: &str = "typeshelf.json";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Export {
    version: u32,
    collections: Vec<ExportCollection>,
    tags: Vec<(String, String)>,
    /// Pinned family keys (missing in backups made before pins).
    #[serde(default)]
    pins: Vec<String>,
    licenses: Vec<ExportLicense>,
    project_sets: Vec<ExportSet>,
    /// Raw JSON, merged over the current settings on restore: a field an older backup lacks keeps its value.
    settings: Option<serde_json::Value>,
    /// Family key → its axis presets (missing in backups made before them: the current ones are kept).
    #[serde(default)]
    axis_presets: Option<BTreeMap<String, serde_json::Value>>,
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
    let mut stmt = conn.prepare("SELECT family_key FROM pins ORDER BY family_key")?;
    let pins = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
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
    let mut stmt = conn.prepare("SELECT substr(key, 14), value FROM settings WHERE key LIKE 'axis\\_presets:%' ESCAPE '\\'")?;
    let rows: Vec<(String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    let axis_presets = rows.into_iter().filter_map(|(k, v)| Some((k, serde_json::from_str(&v).ok()?))).collect();
    Ok(Export {
        version: 1,
        collections,
        tags,
        pins,
        licenses,
        project_sets,
        settings: store::get(conn, "app")?,
        axis_presets: Some(axis_presets),
    })
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

    /// Replaces collections, tags, pins, licenses, project sets and settings; undoable from history.
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
             DELETE FROM collection_items; DELETE FROM collections; DELETE FROM family_tags; DELETE FROM pins;
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
        for k in &e.pins {
            tx.execute("INSERT OR IGNORE INTO pins (family_key) VALUES (?1)", [k])?;
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
        if let Some(serde_json::Value::Object(s)) = &e.settings {
            let mut merged = serde_json::to_value(store::get::<AppSettings>(&tx, "app")?.unwrap_or_default())?;
            if let serde_json::Value::Object(m) = &mut merged {
                m.extend(s.clone());
            }
            let merged: AppSettings =
                serde_json::from_value(merged).map_err(|e| EngineError::new("invalid_backup", e.to_string()))?;
            store::set(&tx, "app", &merged)?;
        }
        if let Some(presets) = &e.axis_presets {
            tx.execute("DELETE FROM settings WHERE key LIKE 'axis\\_presets:%' ESCAPE '\\'", [])?;
            for (family_key, list) in presets {
                store::set(&tx, &format!("axis_presets:{family_key}"), list)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Writes every collection except Recent, nested, with each family's key and name.
    pub fn export_collections(&self, dest: impl AsRef<Path>) -> Result<CollectionsExport> {
        let file = {
            let db = self.db();
            let mut stmt = db.prepare("SELECT id, name, parent_id FROM collections WHERE builtin IS NOT 'recent' ORDER BY id")?;
            let rows: Vec<(i64, String, Option<i64>)> =
                stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
            let mut stmt = db.prepare("SELECT family_key, MIN(family) FROM faces GROUP BY family_key")?;
            let names: HashMap<String, String> =
                stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
            fn node(db: &Connection, rows: &[(i64, String, Option<i64>)], names: &HashMap<String, String>, id: i64, name: &str) -> rusqlite::Result<CollectionNode> {
                let families = strings(db, "SELECT family_key FROM collection_items WHERE collection_id = ?1 ORDER BY added_at", id)?
                    .into_iter()
                    .map(|key| {
                        // A family no longer in the library keeps the name part of its key.
                        let family = names.get(&key).cloned().unwrap_or_else(|| key.split('|').next().unwrap_or_default().into());
                        FamilyRef { key, family }
                    })
                    .collect();
                let children = rows
                    .iter()
                    .filter(|r| r.2 == Some(id))
                    .map(|r| node(db, rows, names, r.0, &r.1))
                    .collect::<rusqlite::Result<_>>()?;
                Ok(CollectionNode { name: name.into(), families, children })
            }
            let collections = rows
                .iter()
                .filter(|r| r.2.is_none_or(|p| !rows.iter().any(|o| o.0 == p)))
                .map(|r| node(&db, &rows, &names, r.0, &r.1))
                .collect::<rusqlite::Result<_>>()?;
            CollectionsFile { version: 1, collections }
        };
        std::fs::write(dest.as_ref(), serde_json::to_vec_pretty(&file)?)?;
        Ok(CollectionsExport { collections: file.collections.iter().map(CollectionNode::count).sum() })
    }

    /// Merges a file from `export_collections`: a collection with the same name under the same
    /// parent is reused, families match by key, then by family name.
    pub fn import_collections(&self, path: impl AsRef<Path>) -> Result<CollectionsImport> {
        let json = std::fs::read(path.as_ref())?;
        let file: CollectionsFile =
            serde_json::from_slice(&json).map_err(|e| EngineError::new("invalid", format!("not a collections file: {e}")))?;
        let mut db = self.db();
        let tx = db.transaction()?;
        let mut keys = HashSet::new();
        let mut by_name = HashMap::new();
        {
            let mut stmt = tx.prepare("SELECT family_key, family FROM faces ORDER BY family_key")?;
            for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (key, family) = row?;
                by_name.entry(family.to_lowercase()).or_insert_with(|| key.clone());
                keys.insert(key);
            }
        }
        let mut res = CollectionsImport { collections: 0, families: 0, missing: 0 };
        fn merge(
            tx: &Connection,
            node: &CollectionNode,
            parent: Option<i64>,
            find: &dyn Fn(&FamilyRef) -> Option<String>,
            res: &mut CollectionsImport,
        ) -> rusqlite::Result<()> {
            let existing: Option<i64> = tx
                .query_row(
                    "SELECT id FROM collections WHERE name = ?1 COLLATE NOCASE AND parent_id IS ?2 ORDER BY id LIMIT 1",
                    params![node.name, parent],
                    |r| r.get(0),
                )
                .optional()?;
            let id = match existing {
                Some(id) => id,
                None => {
                    tx.execute("INSERT INTO collections (name, parent_id) VALUES (?1, ?2)", params![node.name, parent])?;
                    tx.last_insert_rowid()
                }
            };
            res.collections += 1;
            for f in &node.families {
                match find(f) {
                    Some(key) => {
                        tx.execute(
                            "INSERT OR IGNORE INTO collection_items (collection_id, family_key, added_at) VALUES (?1, ?2, ?3)",
                            params![id, key, store::now()],
                        )?;
                        res.families += 1;
                    }
                    None => res.missing += 1,
                }
            }
            node.children.iter().try_for_each(|c| merge(tx, c, Some(id), find, res))
        }
        let find = |f: &FamilyRef| -> Option<String> {
            Some(f.key.clone()).filter(|k| keys.contains(k)).or_else(|| by_name.get(&f.family.trim().to_lowercase()).cloned())
        };
        for node in &file.collections {
            if node.name.trim().is_empty() {
                continue;
            }
            merge(&tx, node, None, &find, &mut res)?;
        }
        tx.commit()?;
        Ok(res)
    }
}

/// The `export_collections` file.
#[derive(Debug, Serialize, Deserialize)]
struct CollectionsFile {
    version: u32,
    collections: Vec<CollectionNode>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CollectionNode {
    name: String,
    #[serde(default)]
    families: Vec<FamilyRef>,
    #[serde(default)]
    children: Vec<CollectionNode>,
}

impl CollectionNode {
    fn count(&self) -> u32 {
        1 + self.children.iter().map(CollectionNode::count).sum::<u32>()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct FamilyRef {
    key: String,
    family: String,
}

fn zip_err(e: zip::result::ZipError) -> EngineError {
    EngineError::new("invalid_backup", e.to_string())
}
