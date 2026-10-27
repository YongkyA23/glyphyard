//! Folder tree (watched folders and their subfolders), turning a whole folder on or off, and
//! auto-activation of fonts that arrive in a folder.

use crate::scan::{folder_rows, SYSTEM};
use crate::{ActivationResult, Engine, FolderNode, Result};
use rusqlite::Connection;
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

fn node(dir: &Path, name: String, hashes: &HashMap<PathBuf, HashSet<String>>, kids: &BTreeMap<PathBuf, Vec<PathBuf>>, active: &HashSet<String>, auto: &HashSet<PathBuf>) -> FolderNode {
    let own = hashes.get(dir);
    let mut children: Vec<FolderNode> = kids
        .get(dir)
        .into_iter()
        .flatten()
        .map(|c| node(c, c.file_name().unwrap_or_default().to_string_lossy().into_owned(), hashes, kids, active, auto))
        .collect();
    children.sort_by_key(|c| c.name.to_lowercase());
    FolderNode {
        path: dir.to_string_lossy().into_owned(),
        name,
        font_count: own.map_or(0, |h| h.len() as u32),
        active_count: own.map_or(0, |h| h.iter().filter(|x| active.contains(*x)).count() as u32),
        auto_activate: auto.contains(dir),
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
        let auto: HashSet<PathBuf> = auto_dirs(&db)?.into_iter().collect();

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
        Ok(roots.iter().map(|r| node(r, r.to_string_lossy().into_owned(), &hashes, &kids, &active, &auto)).collect())
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
    pub fn set_folder_active(&self, path: &str, active: bool, confirm_unknown_license: bool) -> Result<Vec<ActivationResult>> {
        let hashes = self.hashes_under(Path::new(path))?;
        self.set_active(&hashes, active, confirm_unknown_license)
    }

    /// Auto-activate: turning it on also turns on what the folder already holds.
    pub fn set_folder_auto(&self, path: &str, on: bool, confirm_unknown_license: bool) -> Result<Vec<ActivationResult>> {
        let results = if on { self.set_folder_active(path, true, confirm_unknown_license)? } else { Vec::new() };
        let sql = if on { "INSERT OR IGNORE INTO auto_folders (path) VALUES (?1)" } else { "DELETE FROM auto_folders WHERE path = ?1" };
        self.db().execute(sql, [path])?;
        self.library_changed("activation");
        Ok(results)
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
