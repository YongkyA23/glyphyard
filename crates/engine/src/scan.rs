//! Folders (identity, offline/online, relocation) and incremental scanning.

use crate::{not_found, store, Engine, EngineError, Folder, FolderStatus, Result, UiEvent};
use notify_debouncer_full::file_id::{get_file_id, FileId};
use rayon::prelude::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use typeshelf_core::organize::is_junk_path;
use typeshelf_core::{file_hash, parse_faces, FaceInfo};
use walkdir::WalkDir;

const FONT_EXTS: [&str; 6] = ["ttf", "otf", "ttc", "otc", "woff", "woff2"];

/// Families are grouped by family name + foundry.
pub(crate) fn family_key(family: &str, foundry: &str) -> String {
    format!("{}|{}", family.trim().to_lowercase(), foundry.trim().to_lowercase())
}

fn folder_file_id(path: &Path) -> Option<String> {
    Some(match get_file_id(path).ok()? {
        FileId::Inode { device_id, inode_number } => format!("{device_id}:{inode_number}"),
        FileId::LowRes { volume_serial_number, file_index } => format!("{volume_serial_number}:{file_index}"),
        FileId::HighRes { volume_serial_number, file_id } => format!("{volume_serial_number}:{file_id}"),
    })
}

/// Looks for a folder that was renamed or moved nearby, by its file id.
// ponytail: searches two levels below the nearest surviving ancestor; "Locate…" covers moves further away.
fn find_moved(old: &Path, id: &str) -> Option<PathBuf> {
    let base = old.ancestors().skip(1).find(|p| p.is_dir())?;
    WalkDir::new(base)
        .min_depth(1)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| e.file_type().is_dir() && folder_file_id(e.path()).as_deref() == Some(id))
        .map(|e| e.into_path())
}

fn is_candidate(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    FONT_EXTS.contains(&ext.as_str()) || is_junk_path(path)
}

pub(crate) struct FolderRow {
    pub id: i64,
    pub path: PathBuf,
    pub file_id: Option<String>,
    pub online: bool,
    pub offline_since: Option<String>,
}

pub(crate) fn folder_rows(conn: &Connection) -> rusqlite::Result<Vec<FolderRow>> {
    let mut stmt = conn.prepare("SELECT id, path, file_id, status, offline_since FROM folders ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok(FolderRow {
            id: r.get(0)?,
            path: PathBuf::from(r.get::<_, String>(1)?),
            file_id: r.get(2)?,
            online: r.get::<_, String>(3)? != "offline",
            offline_since: r.get(4)?,
        })
    })?;
    rows.collect()
}

fn folder_row(conn: &Connection, id: i64) -> Result<FolderRow> {
    folder_rows(conn)?.into_iter().find(|f| f.id == id).ok_or_else(|| not_found("folder"))
}

fn folder_dto(conn: &Connection, f: &FolderRow, status: Option<FolderStatus>) -> rusqlite::Result<Folder> {
    let font_count: u32 = conn.query_row(
        "SELECT COUNT(DISTINCT hash) FROM files fi WHERE folder_id = ?1 AND EXISTS (SELECT 1 FROM faces WHERE hash = fi.hash)",
        [f.id],
        |r| r.get(0),
    )?;
    Ok(Folder {
        id: f.id,
        path: f.path.to_string_lossy().into_owned(),
        status: status.unwrap_or(if f.online { FolderStatus::Watching } else { FolderStatus::Offline }),
        font_count,
        offline_since: f.offline_since.clone(),
    })
}

pub(crate) fn prune_faces(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM faces WHERE hash NOT IN (SELECT hash FROM files)", [])?;
    Ok(())
}

fn insert_faces(conn: &Connection, hash: &str, faces: &[FaceInfo]) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO faces (hash, idx, family_key, ps_name, family, style, full_name, version, foundry,
         designer, weight, italic, is_variable, axes_json, features_json, glyph_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
    )?;
    for f in faces {
        stmt.execute(params![
            hash,
            f.index,
            family_key(&f.family, &f.foundry),
            f.ps_name,
            f.family,
            f.style,
            f.full_name,
            f.version,
            f.foundry,
            f.designer,
            f.weight,
            f.italic,
            f.is_variable,
            serde_json::to_string(&f.axes).unwrap_or_default(),
            serde_json::to_string(&f.features).unwrap_or_default(),
            f.glyph_count,
        ])?;
    }
    Ok(())
}

struct Scanned {
    rel: String,
    size: i64,
    mtime: i64,
    hash: String,
    format: String,
    faces: Vec<FaceInfo>,
}

impl Engine {
    pub fn list_folders(&self) -> Result<Vec<Folder>> {
        let db = self.db();
        let rows = folder_rows(&db)?;
        Ok(rows.iter().map(|f| folder_dto(&db, f, None)).collect::<rusqlite::Result<_>>()?)
    }

    pub fn add_folder(&self, path: impl AsRef<Path>) -> Result<Vec<Folder>> {
        let id = self.insert_folder(path.as_ref(), None)?;
        self.scan_folder(id)?;
        self.watch_folder(path.as_ref(), true);
        self.library_changed("scan");
        self.list_folders()
    }

    /// Adds a folder (or returns the existing one with the same path).
    pub(crate) fn insert_folder(&self, path: &Path, managed: Option<&str>) -> Result<i64> {
        let path = std::path::absolute(path)?;
        if !path.is_dir() {
            return Err(EngineError::new("not_a_folder", format!("{} is not a folder", path.display())));
        }
        let db = self.db();
        for f in folder_rows(&db)? {
            if f.path == path {
                return Ok(f.id);
            }
            if path.starts_with(&f.path) || f.path.starts_with(&path) {
                return Err(EngineError::new("overlap", format!("{} overlaps {}", path.display(), f.path.display())));
            }
        }
        db.execute(
            "INSERT INTO folders (path, file_id, managed, added_at) VALUES (?1, ?2, ?3, ?4)",
            params![path.to_string_lossy(), folder_file_id(&path), managed, store::now()],
        )?;
        Ok(db.last_insert_rowid())
    }

    pub fn remove_folder(&self, id: i64) -> Result<Vec<Folder>> {
        let path = {
            let db = self.db();
            let folder = folder_row(&db, id)?;
            let active: Vec<String> = self
                .ledger(&db)?
                .into_iter()
                .filter(|a| Path::new(&a.path).starts_with(&folder.path))
                .map(|a| a.hash)
                .collect();
            self.deactivate_hashes(&db, &active, typeshelf_protocol::Source::User)?;
            db.execute("DELETE FROM folders WHERE id = ?1", [id])?;
            prune_faces(&db)?;
            folder.path
        };
        self.watch_folder(&path, false);
        self.library_changed("scan");
        self.list_folders()
    }

    pub fn locate_folder(&self, id: i64, path: impl AsRef<Path>) -> Result<Vec<Folder>> {
        let new = std::path::absolute(path.as_ref())?;
        if !new.is_dir() {
            return Err(EngineError::new("not_a_folder", format!("{} is not a folder", new.display())));
        }
        let old = folder_row(&self.db(), id)?.path;
        self.relocate(id, &old, &new)?;
        self.scan_folder(id)?;
        self.watch_folder(&new, true);
        self.library_changed("scan");
        self.list_folders()
    }

    /// Full rescan: re-checks which folders are online, then scans every online folder.
    pub fn rescan(&self) -> Result<Vec<Folder>> {
        self.refresh_folders()?;
        let ids: Vec<i64> = folder_rows(&self.db())?.into_iter().filter(|f| f.online).map(|f| f.id).collect();
        let mut changed = false;
        for id in ids {
            changed |= self.scan_folder(id)?;
        }
        if changed {
            self.library_changed("scan");
        }
        self.list_folders()
    }

    /// Marks vanished folders offline (keeping their data), follows moved ones and brings back
    /// folders whose path exists again.
    pub(crate) fn refresh_folders(&self) -> Result<()> {
        let rows = folder_rows(&self.db())?;
        for f in rows {
            if f.path.is_dir() {
                if !f.online {
                    self.set_folder_online(f.id, true)?;
                }
            } else if let Some(found) = f.file_id.as_deref().and_then(|id| find_moved(&f.path, id)) {
                self.relocate(f.id, &f.path, &found)?;
            } else if f.online {
                self.set_folder_online(f.id, false)?;
            }
        }
        Ok(())
    }

    fn set_folder_online(&self, id: i64, online: bool) -> Result<()> {
        let folder = {
            let db = self.db();
            if online {
                db.execute("UPDATE folders SET status = 'watching', offline_since = NULL WHERE id = ?1", [id])?;
            } else {
                db.execute("UPDATE folders SET status = 'offline', offline_since = ?2 WHERE id = ?1", params![id, store::now()])?;
            }
            let row = folder_row(&db, id)?;
            folder_dto(&db, &row, None)?
        };
        self.ui(UiEvent::FolderStatus(folder));
        Ok(())
    }

    /// Points a folder at its new location and moves its active fonts' registrations along.
    fn relocate(&self, id: i64, old: &Path, new: &Path) -> Result<()> {
        let folder = {
            let db = self.db();
            db.execute(
                "UPDATE folders SET path = ?2, file_id = ?3, status = 'watching', offline_since = NULL WHERE id = ?1",
                params![id, new.to_string_lossy(), folder_file_id(new)],
            )?;
            let mut moved = false;
            for a in self.ledger(&db)? {
                let Ok(rest) = Path::new(&a.path).strip_prefix(old) else { continue };
                let to = new.join(rest);
                let _ = self.os.deactivate(&a.entry());
                db.execute("UPDATE activations SET path = ?2 WHERE hash = ?1", params![a.hash, to.to_string_lossy()])?;
                let mut entry = a.entry();
                entry.path = to;
                if let Err(e) = self.os.activate(&entry) {
                    self.activation_failed(&db, &a.hash, &e);
                }
                moved = true;
            }
            if moved {
                self.os.notify_font_change();
            }
            let row = folder_row(&db, id)?;
            folder_dto(&db, &row, None)?
        };
        self.ui(UiEvent::FolderStatus(folder));
        Ok(())
    }

    /// Incremental scan: only files whose size or mtime changed are read, hashed and parsed (in
    /// parallel). Returns whether anything changed.
    pub(crate) fn scan_folder(&self, id: i64) -> Result<bool> {
        let (folder, existing) = {
            let db = self.db();
            let folder = folder_row(&db, id)?;
            let mut stmt = db.prepare("SELECT rel_path, size, mtime FROM files WHERE folder_id = ?1")?;
            let existing: HashMap<String, (i64, i64)> = stmt
                .query_map([id], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
                .collect::<rusqlite::Result<_>>()?;
            let dto = folder_dto(&db, &folder, Some(FolderStatus::Scanning))?;
            drop(stmt);
            drop(db);
            self.ui(UiEvent::FolderStatus(dto));
            (folder, existing)
        };
        let root = &folder.path;
        if !root.is_dir() {
            self.set_folder_online(id, false)?;
            return Ok(false);
        }

        let mut seen = HashSet::new();
        let mut todo = Vec::new();
        for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() || !is_candidate(entry.path()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            // ponytail: cloud placeholders (OneDrive "files on demand") are skipped rather than listed unread.
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
                if meta.file_attributes() & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS != 0 {
                    continue;
                }
            }
            let rel = entry.path().strip_prefix(root).unwrap_or(entry.path()).to_string_lossy().into_owned();
            let size = meta.len() as i64;
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            if existing.get(&rel) != Some(&(size, mtime)) {
                todo.push((rel.clone(), entry.into_path(), size, mtime));
            }
            seen.insert(rel);
        }
        if !root.is_dir() {
            // The volume went away mid-scan: keep what we know.
            self.set_folder_online(id, false)?;
            return Ok(false);
        }

        let scanned: Vec<Scanned> = todo
            .par_iter()
            .filter_map(|(rel, path, size, mtime)| {
                let bytes = std::fs::read(path).ok()?;
                Some(Scanned {
                    rel: rel.clone(),
                    size: *size,
                    mtime: *mtime,
                    hash: file_hash(&bytes),
                    format: path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase(),
                    faces: parse_faces(&bytes).unwrap_or_default(),
                })
            })
            .collect();

        let removed: Vec<&String> = existing.keys().filter(|k| !seen.contains(*k)).collect();
        let changed = !scanned.is_empty() || !removed.is_empty();
        let dto = {
            let mut db = self.db();
            let tx = db.transaction()?;
            for rel in &removed {
                tx.execute("DELETE FROM files WHERE folder_id = ?1 AND rel_path = ?2", params![id, rel])?;
            }
            let now = store::now();
            for s in &scanned {
                tx.execute(
                    "INSERT INTO files (folder_id, rel_path, hash, size, mtime, format, added_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT (folder_id, rel_path) DO UPDATE SET
                       hash = excluded.hash, size = excluded.size, mtime = excluded.mtime, format = excluded.format",
                    params![id, s.rel, s.hash, s.size, s.mtime, s.format, now],
                )?;
                insert_faces(&tx, &s.hash, &s.faces)?;
            }
            if !removed.is_empty() {
                prune_faces(&tx)?;
            }
            tx.commit()?;
            let row = folder_row(&db, id)?;
            folder_dto(&db, &row, None)?
        };
        self.ui(UiEvent::FolderStatus(dto));
        Ok(changed)
    }

    /// Folder ids containing any of `paths`.
    pub(crate) fn folders_containing(&self, paths: &[PathBuf]) -> Result<Vec<i64>> {
        let rows = folder_rows(&self.db())?;
        Ok(rows.into_iter().filter(|f| paths.iter().any(|p| p.starts_with(&f.path))).map(|f| f.id).collect())
    }

    pub(crate) fn managed_folder(&self, kind: &str, dir_name: &str) -> Result<(i64, PathBuf)> {
        let found: Option<(i64, String)> = self
            .db()
            .query_row("SELECT id, path FROM folders WHERE managed = ?1", [kind], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        if let Some((id, path)) = found {
            let path = PathBuf::from(path);
            std::fs::create_dir_all(&path)?;
            return Ok((id, path));
        }
        let path = self.data_dir.join(dir_name);
        std::fs::create_dir_all(&path)?;
        let id = self.insert_folder(&path, Some(kind))?;
        self.watch_folder(&path, true);
        Ok((id, path))
    }
}
