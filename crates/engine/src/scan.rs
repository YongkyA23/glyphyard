//! Folders (identity, offline/online, relocation) and incremental scanning.

use crate::organize::to_trash;
use crate::{not_found, store, DeleteResult, Engine, EngineError, Folder, FolderStatus, ImportResult, Result, UiEvent};
use notify_debouncer_full::file_id::{get_file_id, FileId};
use rayon::prelude::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use typeshelf_core::organize::is_junk_path;
use typeshelf_core::{file_hash, glyphs, parse_faces, FaceInfo};
use walkdir::WalkDir;

pub(crate) const FONT_EXTS: [&str; 6] = ["ttf", "otf", "ttc", "otc", "woff", "woff2"];
/// `folders.managed` for the OS font folders: read-only, always on, never organized.
pub(crate) const SYSTEM: &str = "system";
/// The app-managed folders as (`folders.managed` kind, directory name in the data folder), used by
/// `managed_folder` and by `adopt_managed_folders`.
pub(crate) const IMPORTED: (&str, &str) = ("imported", "Imported fonts");
pub(crate) const GOOGLE: (&str, &str) = ("google", "Google Fonts");
pub(crate) const SHARED: (&str, &str) = ("shared", "Shared library");

/// Settings key recording that the user removed that kind's managed folder, so a rescan does not adopt it.
// ponytail: a folder removed before this record existed comes back once, at the first rescan after updating.
fn removed_key(kind: &str) -> String {
    format!("managed_removed.{kind}")
}

/// Families are grouped by family name + foundry.
pub(crate) fn family_key(family: &str, foundry: &str) -> String {
    format!("{}|{}", family.trim().to_lowercase(), foundry.trim().to_lowercase())
}

/// `name.ext` -> `name (n).ext`; n = 1 leaves the name as it is.
pub(crate) fn numbered(path: &Path, n: u32) -> PathBuf {
    if n <= 1 {
        return path.to_path_buf();
    }
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let name = match path.extension() {
        Some(ext) => format!("{stem} ({n}).{}", ext.to_string_lossy()),
        None => format!("{stem} ({n})"),
    };
    path.with_file_name(name)
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

/// A watched folder may not contain, or sit inside, another one: each file would be listed twice
/// and auto-organize would see it as its own duplicate.
// ponytail: compares paths as written (case-sensitive, junctions not resolved); canonicalize if that bites.
fn check_overlap(conn: &Connection, path: &Path, except: Option<i64>) -> Result<()> {
    match folder_rows(conn)?.into_iter().find(|f| Some(f.id) != except && (path.starts_with(&f.path) || f.path.starts_with(path))) {
        Some(f) => Err(EngineError::new("overlap", format!("{} overlaps {}", path.display(), f.path.display()))),
        None => Ok(()),
    }
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

/// Inserts or refreshes the faces of one file (`coverage`: JSON per face); a family override
/// (`set_family_name`) wins over the parsed family name.
fn insert_faces(conn: &Connection, hash: &str, faces: &[FaceInfo], coverage: &[String]) -> rusqlite::Result<()> {
    let over: Option<String> = conn
        .prepare_cached("SELECT family FROM family_overrides WHERE hash = ?1")?
        .query_row([hash], |r| r.get(0))
        .optional()?;
    let mut stmt = conn.prepare_cached(
        "INSERT INTO faces (hash, idx, family_key, ps_name, family, style, full_name, version, foundry,
         designer, weight, italic, is_variable, axes_json, features_json, glyph_count, category, metrics_json, real_family,
         coverage_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
         ON CONFLICT (hash, idx) DO UPDATE SET family_key = excluded.family_key, ps_name = excluded.ps_name,
           family = excluded.family, style = excluded.style, full_name = excluded.full_name, version = excluded.version,
           foundry = excluded.foundry, designer = excluded.designer, weight = excluded.weight, italic = excluded.italic,
           is_variable = excluded.is_variable, axes_json = excluded.axes_json, features_json = excluded.features_json,
           glyph_count = excluded.glyph_count, category = excluded.category, metrics_json = excluded.metrics_json,
           real_family = excluded.real_family, coverage_json = excluded.coverage_json",
    )?;
    for (f, cov) in faces.iter().zip(coverage) {
        let family = over.as_deref().unwrap_or(&f.family);
        stmt.execute(params![
            hash,
            f.index,
            family_key(family, &f.foundry),
            f.ps_name,
            family,
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
            f.metrics.category.map(|c| c.as_str()),
            serde_json::to_string(&f.metrics).unwrap_or_default(),
            f.family,
            cov,
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
    coverage: Vec<String>,
}

impl Engine {
    /// The user's folders; the system font folders are shown as "System fonts" instead.
    pub fn list_folders(&self) -> Result<Vec<Folder>> {
        let db = self.db();
        let system: HashSet<i64> = db
            .prepare("SELECT id FROM folders WHERE managed = ?1")?
            .query_map([SYSTEM], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let rows = folder_rows(&db)?;
        Ok(rows.iter().filter(|f| !system.contains(&f.id)).map(|f| folder_dto(&db, f, None)).collect::<rusqlite::Result<_>>()?)
    }

    /// Every folder the library can list fonts from: the user's, the managed ones and the system ones.
    pub fn folder_paths(&self) -> Result<Vec<PathBuf>> {
        Ok(folder_rows(&self.db())?.into_iter().map(|f| f.path).collect())
    }

    /// Adds the OS font folders the first time they are seen.
    fn ensure_system_folders(&self) {
        for dir in self.os.font_dirs() {
            // ponytail: fonts the registry points at outside these folders are not listed.
            if dir.is_dir() {
                let _ = self.insert_folder(&dir, Some(SYSTEM)); // an overlapping user folder wins
            }
        }
    }

    /// Gives a row (and a watch) to managed folders that are already in the data folder but not in the
    /// database, as after a reinstall that kept them. Absent ones stay lazy: `managed_folder` creates them.
    /// One the user removed stays removed until `managed_folder` needs it again.
    fn adopt_managed_folders(&self) {
        for (kind, name) in [IMPORTED, GOOGLE, SHARED] {
            let path = self.data_dir.join(name);
            let known: rusqlite::Result<bool> = self.db().query_row(
                "SELECT EXISTS(SELECT 1 FROM folders WHERE managed = ?1) OR EXISTS(SELECT 1 FROM settings WHERE key = ?2)",
                params![kind, removed_key(kind)],
                |r| r.get(0),
            );
            if path.is_dir() && known == Ok(false) && self.insert_folder(&path, Some(kind)).is_ok() {
                self.watch_folder(&path, true);
            }
        }
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
        if let Some(f) = folder_rows(&db)?.into_iter().find(|f| f.path == path) {
            return Ok(f.id);
        }
        check_overlap(&db, &path, None)?;
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
            let managed: Option<String> = db.query_row("SELECT managed FROM folders WHERE id = ?1", [id], |r| r.get(0))?;
            if let Some(kind) = managed {
                store::set(&db, &removed_key(&kind), &true)?;
            }
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
        let old = {
            let db = self.db();
            check_overlap(&db, &new, Some(id))?;
            folder_row(&db, id)?.path
        };
        self.relocate(id, &old, &new)?;
        self.scan_folder(id)?;
        self.watch_folder(&new, true);
        self.library_changed("scan");
        self.list_folders()
    }

    /// Full rescan: re-checks which folders are online, then scans every online folder.
    pub fn rescan(&self) -> Result<Vec<Folder>> {
        self.ensure_system_folders();
        self.adopt_managed_folders();
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

    /// Cheap check for a running app to poll: did an offline folder's drive come back? If so its fonts
    /// are rescanned and its active fonts registered again. Returns whether anything came back.
    pub fn reconnect_folders(&self) -> Result<bool> {
        // Rows first, so the DB lock is released before probing paths (an absent share can block for seconds).
        let rows = folder_rows(&self.db())?;
        let back = rows.iter().any(|f| !f.online && f.path.is_dir());
        if back {
            self.rescan()?;
        }
        Ok(back)
    }

    /// Registers the active fonts under `dir` again (startup repair drops registrations whose file
    /// was unreachable).
    fn reregister_under(&self, dir: &Path) -> Result<()> {
        let rows: Vec<_> = self.ledger(&self.db())?.into_iter().filter(|a| Path::new(&a.path).starts_with(dir)).collect();
        let mut any = false;
        for a in &rows {
            match self.os.activate(&a.entry()) {
                Ok(()) => any = true,
                Err(e) => self.activation_failed(&self.db(), &a.hash, &e),
            }
        }
        if any {
            self.os.notify_font_change();
        }
        Ok(())
    }

    /// Marks vanished folders offline (keeping their data), follows moved ones and brings back
    /// folders whose path exists again.
    pub(crate) fn refresh_folders(&self) -> Result<()> {
        let rows = folder_rows(&self.db())?;
        for f in rows {
            if f.path.is_dir() {
                if !f.online {
                    self.set_folder_online(f.id, true)?;
                    self.reregister_under(&f.path)?;
                    self.watch_folder(&f.path, true);
                }
            } else if let Some(found) = f
                .file_id
                .as_deref()
                .and_then(|id| find_moved(&f.path, id))
                // Moved inside another watched folder: that folder lists the files already.
                .filter(|found| check_overlap(&self.db(), found, Some(f.id)).is_ok())
            {
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
            let mut stmt = db.prepare("SELECT rel_path, size, mtime, hash FROM files WHERE folder_id = ?1")?;
            let existing: HashMap<String, (i64, i64, String)> = stmt
                .query_map([id], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?
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
        // Places that could not be read this time (no access, a network hiccup): what is known under them stays.
        let mut unread: Vec<PathBuf> = Vec::new();
        for entry in WalkDir::new(root) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    unread.push(e.path().unwrap_or(root).to_path_buf());
                    continue;
                }
            };
            if !entry.file_type().is_file() || !is_candidate(entry.path()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                unread.push(entry.into_path());
                continue;
            };
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
            if existing.get(&rel).is_none_or(|e| (e.0, e.1) != (size, mtime)) {
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
                let faces = parse_faces(&bytes).unwrap_or_default();
                let coverage = faces.iter().map(|f| serde_json::to_string(&glyphs::coverage(&bytes, f.index)).unwrap_or_default()).collect();
                Some(Scanned {
                    rel: rel.clone(),
                    size: *size,
                    mtime: *mtime,
                    hash: file_hash(&bytes),
                    format: path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase(),
                    faces,
                    coverage,
                })
            })
            .collect();

        let removed: Vec<&String> =
            existing.keys().filter(|k| !seen.contains(*k) && !unread.iter().any(|u| root.join(k).starts_with(u))).collect();
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
                insert_faces(&tx, &s.hash, &s.faces, &s.coverage)?;
            }
            tx.commit()?;
            let row = folder_row(&db, id)?;
            folder_dto(&db, &row, None)?
        };
        self.ui(UiEvent::FolderStatus(dto));
        // Before pruning faces, so a font turned off here is still reported by name.
        let changed = self.reconcile_ledger(id, root, &unread)? || changed;
        if !removed.is_empty() {
            prune_faces(&self.db())?;
        }
        // Bytes this folder already had did not arrive: a re-read (touched, re-parsed after an upgrade),
        // a rename, or a move inside the folder.
        let known: HashSet<&str> = existing.values().map(|e| e.2.as_str()).collect();
        let arrived: Vec<(PathBuf, String)> = scanned
            .iter()
            .filter(|s| !s.faces.is_empty() && !known.contains(s.hash.as_str()))
            .map(|s| (root.join(&s.rel), s.hash.clone()))
            .collect();
        if !arrived.is_empty() {
            self.auto_activate(&arrived)?;
        }
        Ok(changed)
    }

    /// Folder ids containing any of `paths`.
    pub(crate) fn folders_containing(&self, paths: &[PathBuf]) -> Result<Vec<i64>> {
        let rows = folder_rows(&self.db())?;
        Ok(rows.into_iter().filter(|f| paths.iter().any(|p| p.starts_with(&f.path))).map(|f| f.id).collect())
    }

    /// Copies dropped font files and folders (recursively; a folder keeps its name as a subfolder)
    /// into "Imported fonts", skipping files whose bytes are already in the library and never
    /// overwriting, then scans it.
    pub fn import_paths(&self, paths: &[String]) -> Result<ImportResult> {
        let (id, root) = self.managed_folder(IMPORTED.0, IMPORTED.1)?;
        let mut known: HashSet<String> =
            self.db().prepare("SELECT DISTINCT hash FROM files")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let mut res = ImportResult { imported: 0, skipped: 0, folder: root.to_string_lossy().into_owned() };
        for p in paths.iter().map(Path::new) {
            let (base, dest_dir) = if p.is_dir() {
                (p, root.join(p.file_name().unwrap_or_default()))
            } else {
                (p.parent().unwrap_or(p), root.clone())
            };
            for entry in WalkDir::new(p).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
                let ext = entry.path().extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                if !FONT_EXTS.contains(&ext.as_str()) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(entry.path()) else {
                    res.skipped += 1;
                    continue;
                };
                if parse_faces(&bytes).is_err() || !known.insert(file_hash(&bytes)) {
                    res.skipped += 1;
                    continue;
                }
                let dest = dest_dir.join(entry.path().strip_prefix(base).unwrap_or(entry.path()));
                let dest = (1..).map(|n| numbered(&dest, n)).find(|d| !d.exists()).unwrap_or(dest);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&dest, &bytes)?;
                res.imported += 1;
            }
        }
        if res.imported > 0 {
            self.scan_folder(id)?;
            self.library_changed("scan");
        }
        Ok(res)
    }

    /// Turns the files off, then sends every copy in the user's folders (and the Imported / Google
    /// folders) to the Recycle Bin. OS font folders and the shared library are never touched.
    pub fn delete_fonts(&self, hashes: &[String]) -> Result<DeleteResult> {
        let copies: Vec<(i64, PathBuf)> = {
            let db = self.db();
            self.deactivate_hashes(&db, hashes, typeshelf_protocol::Source::User)?;
            let mut stmt = db.prepare(
                "SELECT fo.id, fo.path, fi.rel_path FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                 WHERE fi.hash = ?1 AND (fo.managed IS NULL OR fo.managed IN ('imported', 'google'))",
            )?;
            let mut copies = Vec::new();
            for h in hashes {
                let rows = stmt.query_map([h], |r| Ok((r.get(0)?, Path::new(&r.get::<_, String>(1)?).join(r.get::<_, String>(2)?))))?;
                copies.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
            }
            copies
        };
        let mut res = DeleteResult { trashed: 0, failed: Vec::new() };
        let mut folders = BTreeSet::new();
        for (id, path) in copies {
            folders.insert(id);
            match to_trash(&path) {
                Ok(()) => res.trashed += 1,
                Err(e) => res.failed.push(format!("{}: {e}", path.display())),
            }
        }
        for id in folders {
            self.scan_folder(id)?;
        }
        self.library_changed("scan");
        Ok(res)
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
        store::remove(&self.db(), &removed_key(kind))?;
        self.watch_folder(&path, true);
        Ok((id, path))
    }
}
