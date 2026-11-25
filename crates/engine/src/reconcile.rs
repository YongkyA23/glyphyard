//! Brings the activation ledger in line with the files table after a folder scan: a font the user moved
//! follows its file, a font whose file is gone or now holds other bytes is turned off and reported.

use crate::activation::{font_file, font_ref, parse_source_opt, LedgerRow};
use crate::{Engine, Result};
use rusqlite::{params, OptionalExtension};
use std::path::{Path, PathBuf};
use typeshelf_activation::LedgerEntry;
use typeshelf_protocol::{Event, Source};

enum Fix {
    /// Same bytes found at another path in a watched folder.
    Follow(LedgerRow, PathBuf),
    /// Gone (no copy left) or replaced by other bytes (`replaced`).
    Off(LedgerRow, bool),
}

/// What `scan_files` leaves for reconciliation of one scanned (online) folder.
pub(crate) struct ToReconcile {
    pub id: i64,
    pub root: PathBuf,
    /// Not readable this scan: ledger rows under these stay as they are.
    pub unread: Vec<PathBuf>,
    /// Files were removed, so faces need pruning.
    pub removed: bool,
}

impl Engine {
    /// Scans `ids`, then reconciles the ledger once for all of them, so a font moved from one folder to another
    /// is found in the other whatever the scan order. Faces are pruned last, so a font turned off is reported by name.
    pub(crate) fn scan_folders(&self, ids: &[i64]) -> Result<bool> {
        let mut changed = false;
        let mut scanned = Vec::new();
        for &id in ids {
            let (c, pending) = self.scan_files(id)?;
            changed |= c;
            scanned.extend(pending);
        }
        for s in &scanned {
            changed |= self.reconcile_ledger(s.id, &s.root, &s.unread)?;
        }
        if scanned.iter().any(|s| s.removed) {
            crate::scan::prune_faces(&self.db())?;
        }
        Ok(changed)
    }

    /// Ledger rows under `root` (folder `id`, just scanned) whose file is not where the ledger says. Rows under
    /// `unread` (not readable this scan) are left alone. Returns whether a row changed.
    pub(crate) fn reconcile_ledger(&self, id: i64, root: &Path, unread: &[PathBuf]) -> Result<bool> {
        let fixes = {
            let db = self.db();
            let root_str = root.to_string_lossy();
            let mut stmt = db.prepare("SELECT hash, path, full_name, source FROM activations WHERE substr(path, 1, length(?1)) = ?1")?;
            let rows: Vec<(String, String, String, String)> =
                stmt.query_map([&root_str], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?;
            let mut fixes = Vec::new();
            for (hash, path, full_name, source) in rows {
                let Ok(rel) = Path::new(&path).strip_prefix(root) else { continue };
                if unread.iter().any(|u| Path::new(&path).starts_with(u)) {
                    continue;
                }
                let listed: Option<String> = db
                    .query_row("SELECT hash FROM files WHERE folder_id = ?1 AND rel_path = ?2", params![id, rel.to_string_lossy()], |r| r.get(0))
                    .optional()?;
                let row = LedgerRow { hash, path, full_name, source: parse_source_opt(Some(source)).unwrap_or(Source::User) };
                match listed {
                    Some(h) if h == row.hash => {}
                    Some(_) => fixes.push(Fix::Off(row, true)),
                    None => match font_file(&db, &row.hash)?.and_then(|f| f.path) {
                        Some(to) => fixes.push(Fix::Follow(row, to)),
                        None => fixes.push(Fix::Off(row, false)),
                    },
                }
            }
            fixes
        };
        if fixes.is_empty() {
            return Ok(false);
        }
        let mut changed = false;
        for fix in fixes {
            match fix {
                // Still on disk but not listed (a cloud placeholder): nothing is known to be wrong with it.
                Fix::Follow(row, _) | Fix::Off(row, false) if Path::new(&row.path).is_file() => {}
                Fix::Follow(row, to) => {
                    self.follow(&row, to)?;
                    changed = true;
                }
                Fix::Off(row, _) => {
                    // ponytail: `Source` has no "system" value, so the row's own source is used (an admin font stays
                    // Admin, so the server sees no user override); a font the user turned on goes off as `User`, which
                    // usage statistics would count as the user's action. A turned-off admin font is attributed to the
                    // admin on the server timeline, and a replaced file is reported as "file_missing" too. Upgrade path: a
                    // dedicated source and reason in the protocol.
                    let (_, off) = self.deactivate_unlocked(std::slice::from_ref(&row.hash), row.source)?;
                    if off {
                        let db = self.db();
                        self.emit(Event::ActivationFailed { font: font_ref(&db, &row.hash), reason: "file_missing".into() });
                    }
                    changed |= off;
                }
            }
        }
        if changed {
            self.os.notify_font_change();
        }
        Ok(changed)
    }

    /// Registers `row`'s font at `to` and points the ledger there (OS calls without the guard).
    pub(crate) fn follow(&self, row: &LedgerRow, to: PathBuf) -> Result<()> {
        let new = LedgerEntry { path: to, full_name: row.full_name.clone() };
        let _ = self.os.deactivate(&row.entry());
        let registered = self.os.activate(&new);
        let db = self.db();
        let moved = db.execute(
            // `?3` too: another reconciliation may have followed the same row already.
            "UPDATE activations SET path = ?3 WHERE hash = ?1 AND path IN (?2, ?3)",
            params![row.hash, row.path, new.path.to_string_lossy()],
        )?;
        match registered {
            Err(e) => self.activation_failed(&db, &row.hash, &e),
            // Turned off meanwhile: undo the registration.
            Ok(()) if moved == 0 => {
                drop(db);
                let _ = self.os.deactivate(&new);
            }
            Ok(()) => {}
        }
        Ok(())
    }
}
