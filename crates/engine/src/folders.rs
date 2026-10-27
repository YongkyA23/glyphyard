//! Folder tree (watched folders and their subfolders), turning a whole folder on or off, and
//! auto-activation of fonts that arrive in a folder.

use crate::library::hidden_dirs;
use crate::organize::to_trash;
use crate::scan::{folder_rows, SYSTEM};
use crate::{not_found, ActivationResult, DeleteResult, Engine, EngineError, FolderNode, Result};
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use typeshelf_protocol::Source;

/// (file path, hash) of every font file outside the system font folders.
fn font_files(conn: &Connection) -> rusqlite::Result<Vec<(PathBuf, String)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT fo.path, fi.rel_path, fi.hash FROM files fi JOIN folders fo ON fo.id = fi.folder_id
         WHERE fo.managed IS NOT 'system' AND EXISTS (SELECT 1 FROM faces WHERE hash = fi.hash)",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((Path::new(&r.get::<_, String>(0)?).join(r.get::<_, String>(1)?), r.get::<_, String>(2)?))
    })?;
    rows.collect()
}

fn auto_dirs(conn: &Connection) -> rusqlite::Result<Vec<PathBuf>> {
    let mut stmt = conn.prepare_cached("SELECT path FROM auto_folders")?;
    let rows = stmt.query_map([], |r| Ok(PathBuf::from(r.get::<_, String>(0)?)))?;
    rows.collect()
}

/// Per-folder flags shown in the tree.
struct Marks {
    active: HashSet<String>,
    auto: HashSet<PathBuf>,
    hidden: HashSet<PathBuf>,
}

fn node(dir: &Path, name: String, hashes: &HashMap<PathBuf, HashSet<String>>, kids: &BTreeMap<PathBuf, Vec<PathBuf>>, marks: &Marks) -> FolderNode {
    let own = hashes.get(dir);
    let mut children: Vec<FolderNode> = kids
        .get(dir)
        .into_iter()
        .flatten()
        .map(|c| node(c, c.file_name().unwrap_or_default().to_string_lossy().into_owned(), hashes, kids, marks))
        .collect();
    children.sort_by_key(|c| c.name.to_lowercase());
    FolderNode {
        path: dir.to_string_lossy().into_owned(),
        name,
        font_count: own.map_or(0, |h| h.len() as u32),
        active_count: own.map_or(0, |h| h.iter().filter(|x| marks.active.contains(*x)).count() as u32),
        auto_activate: marks.auto.contains(dir),
        hidden: marks.hidden.contains(dir),
        children,
    }
}

impl Engine {
    /// Watched folders (not the system ones) with every subfolder that holds fonts.
    pub fn folder_tree(&self) -> Result<Vec<FolderNode>> {
        let db = self.db();
        let roots: Vec<PathBuf> = {
            let system: HashSet<i64> = db
                .prepare("SELECT id FROM folders WHERE managed = ?1")?
                .query_map([SYSTEM], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            folder_rows(&db)?.into_iter().filter(|f| !system.contains(&f.id)).map(|f| f.path).collect()
        };
        let active: HashSet<String> = db
            .prepare("SELECT hash FROM activations")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let marks = Marks {
            active,
            auto: auto_dirs(&db)?.into_iter().collect(),
            hidden: hidden_dirs(&db)?.into_iter().map(PathBuf::from).collect(),
        };

        // Each file counts toward every folder from its own up to its watched root.
        let mut hashes: HashMap<PathBuf, HashSet<String>> = HashMap::new();
        let mut kids: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
        for (file, hash) in font_files(&db)? {
            let Some(root) = roots.iter().find(|r| file.starts_with(r)) else { continue };
            let mut dir = file.parent();
            while let Some(d) = dir {
                let first = !hashes.contains_key(d);
                hashes.entry(d.to_path_buf()).or_default().insert(hash.clone());
                if d == root.as_path() {
                    break;
                }
                if first {
                    if let Some(parent) = d.parent() {
                        kids.entry(parent.to_path_buf()).or_default().push(d.to_path_buf());
                    }
                }
                dir = d.parent();
            }
        }
        Ok(roots.iter().map(|r| node(r, r.to_string_lossy().into_owned(), &hashes, &kids, &marks)).collect())
    }

    fn hashes_under(&self, dir: &Path) -> Result<Vec<String>> {
        let mut seen = HashSet::new();
        Ok(font_files(&self.db())?
            .into_iter()
            .filter(|(f, h)| f.starts_with(dir) && seen.insert(h.clone()))
            .map(|(_, h)| h)
            .collect())
    }

    /// Turns every font in `path` (and its subfolders) on or off.
    pub fn set_folder_active(&self, path: &str, active: bool) -> Result<Vec<ActivationResult>> {
        let hashes = self.hashes_under(Path::new(path))?;
        self.set_active(&hashes, active)
    }

    /// Auto-activate: turning it on also turns on what the folder already holds.
    pub fn set_folder_auto(&self, path: &str, on: bool) -> Result<Vec<ActivationResult>> {
        let results = if on { self.set_folder_active(path, true)? } else { Vec::new() };
        let sql = if on { "INSERT OR IGNORE INTO auto_folders (path) VALUES (?1)" } else { "DELETE FROM auto_folders WHERE path = ?1" };
        self.db().execute(sql, [path])?;
        self.library_changed("activation");
        Ok(results)
    }

    /// Hidden folders keep their fonts out of the library lists (`FamilyQuery::include_hidden`).
    pub fn set_folder_hidden(&self, path: &str, hidden: bool) -> Result<Vec<FolderNode>> {
        let sql = if hidden { "INSERT OR IGNORE INTO hidden_folders (path) VALUES (?1)" } else { "DELETE FROM hidden_folders WHERE path = ?1" };
        self.db().execute(sql, [path])?;
        self.library_changed("scan");
        self.folder_tree()
    }

    /// Folder right-click > Delete: turns off the fonts in it, sends the folder to the Recycle Bin,
    /// then drops a watched root from the library or rescans the root of a subfolder.
    /// `trashed` counts the font files that went with it.
    pub fn delete_folder(&self, path: &str) -> Result<DeleteResult> {
        let dir = PathBuf::from(path);
        let (id, root, managed): (i64, PathBuf, Option<String>) = {
            let db = self.db();
            let mut stmt = db.prepare("SELECT id, path, managed FROM folders")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, PathBuf::from(r.get::<_, String>(1)?), r.get(2)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().find(|f| dir.starts_with(&f.1)).ok_or_else(|| not_found("folder"))?
        };
        if matches!(managed.as_deref(), Some(SYSTEM | "shared")) {
            return Err(EngineError::new("not_allowed", "system and shared-library folders cannot be deleted here"));
        }
        let trashed = {
            let db = self.db();
            let active: Vec<String> =
                self.ledger(&db)?.into_iter().filter(|a| Path::new(&a.path).starts_with(&dir)).map(|a| a.hash).collect();
            self.deactivate_hashes(&db, &active, Source::User)?;
            font_files(&db)?.iter().filter(|(f, _)| f.starts_with(&dir)).count() as u32
        };
        if let Err(e) = to_trash(&dir) {
            return Ok(DeleteResult { trashed: 0, failed: vec![format!("{}: {e}", dir.display())] });
        }
        {
            let db = self.db();
            for table in ["auto_folders", "hidden_folders"] {
                let paths: Vec<String> =
                    db.prepare(&format!("SELECT path FROM {table}"))?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                for p in paths.into_iter().filter(|p| Path::new(p).starts_with(&dir)) {
                    db.execute(&format!("DELETE FROM {table} WHERE path = ?1"), params![p])?;
                }
            }
        }
        if dir == root {
            self.remove_folder(id)?;
        } else {
            self.scan_folder(id)?;
            self.library_changed("scan");
        }
        Ok(DeleteResult { trashed, failed: Vec::new() })
    }

    /// Called after a scan: turns on new files that landed in an auto-activate folder.
    pub(crate) fn auto_activate(&self, files: &[(PathBuf, String)]) -> Result<bool> {
        let db = self.db();
        let dirs = auto_dirs(&db)?;
        let hashes: Vec<String> =
            files.iter().filter(|(f, _)| dirs.iter().any(|d| f.starts_with(d))).map(|(_, h)| h.clone()).collect();
        if hashes.is_empty() {
            return Ok(false);
        }
        // The user opted in for this folder, so no unknown-license prompt here.
        self.activate_hashes(&db, &hashes, Source::User)?;
        Ok(true)
    }
}
