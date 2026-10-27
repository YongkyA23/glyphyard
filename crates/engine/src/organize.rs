//! Auto-organize apply + the undo journal. The plan itself comes from `typeshelf_core::organize`.

use crate::{store, Engine, EngineError, HistoryEntry, OrganizeChange, OrganizePlan, OrganizeResult, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use typeshelf_core::organize::{plan, Action, Case, Options, ScannedFile};
use typeshelf_core::{file_hash, FaceInfo};
use typeshelf_protocol::Event;

/// What was actually done, in order, so it can be reversed.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Done {
    /// An exact copy went to the Recycle Bin; `kept` has the same bytes, so undo copies it back.
    Trashed { path: PathBuf, kept: PathBuf },
    Moved { from: PathBuf, to: PathBuf, override_hash: Option<String> },
}

fn change_id(c: &typeshelf_core::organize::Change) -> i64 {
    let json = serde_json::to_vec(c).unwrap_or_default();
    // 52 bits so the id stays exact as a JS number.
    i64::from_str_radix(&file_hash(&json)[..13], 16).unwrap_or(0)
}

/// Sends a file or a folder to the Recycle Bin / Trash.
pub(crate) fn to_trash(path: &Path) -> std::io::Result<()> {
    // Tests must not fill the real Recycle Bin.
    if cfg!(test) {
        return if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    }
    trash::delete(path).map_err(std::io::Error::other)
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("{} already exists", to.display())));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(from, to).is_err() {
        // Different volume: copy, then remove.
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)?;
    }
    Ok(())
}

fn scanned_files(conn: &Connection) -> rusqlite::Result<Vec<ScannedFile>> {
    let mut stmt = conn.prepare(
        "SELECT hash, idx, ps_name, family, style, full_name, version, foundry, designer, weight, italic,
                is_variable, axes_json, features_json, glyph_count FROM faces ORDER BY hash, idx",
    )?;
    let mut faces: HashMap<String, Vec<FaceInfo>> = HashMap::new();
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            FaceInfo {
                index: r.get(1)?,
                ps_name: r.get(2)?,
                family: r.get(3)?,
                style: r.get(4)?,
                full_name: r.get(5)?,
                version: r.get(6)?,
                foundry: r.get(7)?,
                designer: r.get(8)?,
                weight: r.get(9)?,
                italic: r.get(10)?,
                is_variable: r.get(11)?,
                axes: serde_json::from_str(&r.get::<_, String>(12)?).unwrap_or_default(),
                features: serde_json::from_str(&r.get::<_, String>(13)?).unwrap_or_default(),
                glyph_count: r.get(14)?,
                metrics: Default::default(),
            },
        ))
    })?;
    for row in rows {
        let (hash, face) = row?;
        faces.entry(hash).or_default().push(face);
    }
    // App-managed folders (shared library, Google Fonts) are left alone.
    let mut stmt = conn.prepare(
        "SELECT fo.path, fi.rel_path, fi.hash, fi.mtime FROM files fi JOIN folders fo ON fo.id = fi.folder_id
         WHERE fo.status != 'offline' AND fo.managed IS NULL ORDER BY fi.id",
    )?;
    let rows = stmt.query_map([], |r| {
        let home = PathBuf::from(r.get::<_, String>(0)?);
        let hash: String = r.get(2)?;
        Ok(ScannedFile {
            path: home.join(r.get::<_, String>(1)?),
            faces: faces.get(&hash).cloned().unwrap_or_default(),
            hash,
            modified: r.get(3)?,
            home: Some(home),
        })
    })?;
    rows.collect()
}

impl Engine {
    fn organize_options(&self, tidy: bool) -> Options {
        Options { tidy, quarantine_dir: self.data_dir.join("Quarantine"), aside_dir: self.data_dir.join("Web fonts") }
    }

    pub fn organize_plan(&self, tidy: bool) -> Result<OrganizePlan> {
        let files = scanned_files(&self.db())?;
        let (changes, notes): (Vec<OrganizeChange>, Vec<OrganizeChange>) = plan(&files, self.system_names(), &self.organize_options(tidy))
            .into_iter()
            .map(|change| OrganizeChange { id: change_id(&change), change })
            .partition(|c| c.change.action.changes_files());
        let count = |case| changes.iter().chain(&notes).filter(|c| c.change.case == case).count() as u32;
        self.emit(Event::DuplicatesReport {
            duplicates: count(Case::ExactCopy),
            version_conflicts: count(Case::DifferentVersion),
            name_clashes: count(Case::NameClash),
        });
        *self.last_plan.lock().unwrap() = Some((tidy, changes.clone()));
        Ok(OrganizePlan { changes, notes, scanned: files.len() as u32, scanned_at: store::now() })
    }

    /// Applies the previewed plan (or a fresh one) minus `exclude_ids`. A failed change is skipped.
    pub fn organize_apply(&self, tidy: bool, exclude_ids: &[i64]) -> Result<OrganizeResult> {
        let cached = self.last_plan.lock().unwrap().take().filter(|(t, _)| *t == tidy).map(|(_, c)| c);
        let changes = match cached {
            Some(c) => c,
            None => self.organize_plan(tidy)?.changes,
        };
        let mut done = Vec::new();
        let mut failed = Vec::new();
        {
            let db = self.db();
            for c in changes.iter().filter(|c| !exclude_ids.contains(&c.id)) {
                let step = match &c.change.action {
                    Action::Trash { path, kept } => self.trash_copy(&db, path, kept),
                    Action::Rename { from, to } => self.rename_with_override(&db, from, to, c.change.case),
                    Action::Move { from, to } | Action::MoveAside { from, to } | Action::Quarantine { from, to } => {
                        self.move_tracked(&db, from, to).map(|_| Done::Moved { from: from.clone(), to: to.clone(), override_hash: None })
                    }
                    Action::KeepAsAlternate { .. } | Action::Group { .. } | Action::Skip { .. } => continue,
                };
                match step {
                    Ok(d) => done.push(d),
                    Err(e) if failed.len() < 20 => failed.push(format!("{}: {}", c.change.action.path().display(), e.message)),
                    Err(_) => {}
                }
            }
            self.os.notify_font_change();
        }
        let applied = done.len() as u32;
        let history_id = {
            let db = self.db();
            db.execute(
                "INSERT INTO history (kind, summary, payload_json, at) VALUES ('organize', ?1, ?2, ?3)",
                params![format!("Auto-organize: {applied} changes"), serde_json::to_string(&done)?, store::now()],
            )?;
            db.last_insert_rowid()
        };
        self.emit(Event::OrganizeApplied { changes: applied });
        self.rescan()?;
        self.library_changed("organize");
        Ok(OrganizeResult { history_id, applied, failed })
    }

    fn trash_copy(&self, conn: &Connection, path: &Path, kept: &Path) -> Result<Done> {
        // If the copy going away is the registered one, register the kept copy instead.
        if let Some((hash, full_name)) = active_at(conn, path)? {
            let old = typeshelf_activation::LedgerEntry { path: path.into(), full_name: full_name.clone() };
            let new = typeshelf_activation::LedgerEntry { path: kept.into(), full_name };
            self.os.deactivate(&old).map_err(|e| EngineError::new("in_use", format!("{e:?}")))?;
            if let Err(e) = self.os.activate(&new) {
                self.activation_failed(conn, &hash, &e);
            }
            conn.execute("UPDATE activations SET path = ?2 WHERE hash = ?1", params![hash, kept.to_string_lossy()])?;
        }
        to_trash(path)?;
        Ok(Done::Trashed { path: path.into(), kept: kept.into() })
    }

    fn rename_with_override(&self, conn: &Connection, from: &Path, to: &Path, case: Case) -> Result<Done> {
        self.move_tracked(conn, from, to)?;
        let mut override_hash = None;
        if matches!(case, Case::DifferentVersion | Case::NameClash) {
            // Show "Name v1.2 (Foundry)" so versions and clashes are told apart in the library.
            let hash = file_hash(&std::fs::read(to)?);
            let row: Option<(String, String, String)> = conn
                .query_row("SELECT hash, full_name, ps_name FROM faces WHERE hash = ?1 ORDER BY idx LIMIT 1", [&hash], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .optional()?;
            if let Some((hash, full_name, ps)) = row {
                let stem = to.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                let display = match stem.strip_prefix(typeshelf_core::organize::sanitize(&ps).as_str()) {
                    Some(rest) => format!("{full_name}{rest}"),
                    None => stem.to_string(),
                };
                conn.execute(
                    "INSERT INTO display_overrides (hash, display_name) VALUES (?1, ?2)
                     ON CONFLICT (hash) DO UPDATE SET display_name = excluded.display_name",
                    params![hash, display],
                )?;
                override_hash = Some(hash);
            }
        }
        Ok(Done::Moved { from: from.into(), to: to.into(), override_hash })
    }

    /// Unregisters an active file before moving it (Windows locks loaded fonts), then registers it
    /// again at the new path.
    fn move_tracked(&self, conn: &Connection, from: &Path, to: &Path) -> Result<()> {
        let active = active_at(conn, from)?;
        if let Some((_, full_name)) = &active {
            let entry = typeshelf_activation::LedgerEntry { path: from.into(), full_name: full_name.clone() };
            self.os.deactivate(&entry).map_err(|e| EngineError::new("in_use", format!("{e:?}")))?;
        }
        let moved = move_file(from, to);
        if let Some((hash, full_name)) = active {
            let at = if moved.is_ok() { to } else { from };
            let entry = typeshelf_activation::LedgerEntry { path: at.into(), full_name };
            if let Err(e) = self.os.activate(&entry) {
                self.activation_failed(conn, &hash, &e);
            }
            conn.execute("UPDATE activations SET path = ?2 WHERE hash = ?1", params![hash, at.to_string_lossy()])?;
        }
        Ok(moved?)
    }

    pub fn list_history(&self) -> Result<Vec<HistoryEntry>> {
        let db = self.db();
        let mut stmt = db.prepare("SELECT id, kind, summary, at, undone FROM history ORDER BY id DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok(HistoryEntry { id: r.get(0)?, kind: r.get(1)?, summary: r.get(2)?, at: r.get(3)?, undone: r.get(4)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn undo_history(&self, id: i64) -> Result<Vec<HistoryEntry>> {
        let (kind, payload, undone): (String, String, bool) = self
            .db()
            .query_row("SELECT kind, payload_json, undone FROM history WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?
            .ok_or_else(|| crate::not_found("history entry"))?;
        if undone {
            return Err(EngineError::new("already_undone", "this change was already undone"));
        }
        match kind.as_str() {
            "organize" => {
                let done: Vec<Done> = serde_json::from_str(&payload)?;
                let db = self.db();
                for d in done.iter().rev() {
                    // Best effort: a file the user moved since stays where it is.
                    let _ = match d {
                        Done::Trashed { path, kept } if !path.exists() => std::fs::copy(kept, path).map(|_| ()).map_err(EngineError::from),
                        Done::Trashed { .. } => Ok(()),
                        Done::Moved { from, to, override_hash } => {
                            if let Some(h) = override_hash {
                                db.execute("DELETE FROM display_overrides WHERE hash = ?1", [h])?;
                            }
                            self.move_tracked(&db, to, from)
                        }
                    };
                }
                self.os.notify_font_change();
            }
            "restore" => self.import_backup(&serde_json::from_str(&payload)?)?,
            other => return Err(EngineError::new("invalid", format!("cannot undo {other}"))),
        }
        self.db().execute("UPDATE history SET undone = 1 WHERE id = ?1", [id])?;
        self.rescan()?;
        self.library_changed("organize");
        self.list_history()
    }
}

/// The ledger row registered at exactly this path.
fn active_at(conn: &Connection, path: &Path) -> rusqlite::Result<Option<(String, String)>> {
    conn.query_row("SELECT hash, full_name FROM activations WHERE path = ?1", [path.to_string_lossy()], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .optional()
}
