//! Activation orchestration: the ledger, one active file per PostScript name, blocking, license
//! checks, project sets and self-repair. The OS is reached only through [`OsFonts`].

use crate::{
    store, ActivationErrorCode, ActivationResult, Engine, EngineError, Health, Problem, Result, UiEvent,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use typeshelf_activation::{ActivationError, LedgerEntry, ReconcileReport};
use typeshelf_protocol::{ActiveFont, Event, FontRef, FontSnapshot, Source};

/// The OS font registration API. [`SystemFonts`] is the real one.
pub trait OsFonts: Send + Sync {
    fn activate(&self, entry: &LedgerEntry) -> Result<(), ActivationError>;
    fn deactivate(&self, entry: &LedgerEntry) -> Result<(), ActivationError>;
    fn reconcile(&self, ledger: &[LedgerEntry]) -> ReconcileReport;
    fn installed_ps_names(&self) -> HashSet<String>;
    fn notify_font_change(&self);
    /// Folders the OS installs fonts into; listed read-only as "System fonts".
    fn font_dirs(&self) -> Vec<PathBuf> {
        Vec::new()
    }
}

pub struct SystemFonts;

impl OsFonts for SystemFonts {
    fn activate(&self, entry: &LedgerEntry) -> Result<(), ActivationError> {
        typeshelf_activation::activate(entry)
    }
    fn deactivate(&self, entry: &LedgerEntry) -> Result<(), ActivationError> {
        typeshelf_activation::deactivate(entry)
    }
    fn reconcile(&self, ledger: &[LedgerEntry]) -> ReconcileReport {
        typeshelf_activation::reconcile(ledger)
    }
    fn installed_ps_names(&self) -> HashSet<String> {
        typeshelf_activation::installed_ps_names()
    }
    fn notify_font_change(&self) {
        typeshelf_activation::notify_font_change()
    }
    fn font_dirs(&self) -> Vec<PathBuf> {
        typeshelf_activation::system_font_dirs()
    }
}

pub(crate) fn source_str(s: Source) -> &'static str {
    match s {
        Source::User => "user",
        Source::Admin => "admin",
        Source::ProjectSet => "project_set",
        Source::Library => "library",
    }
}

fn parse_source(s: &str) -> Source {
    match s {
        "admin" => Source::Admin,
        "project_set" => Source::ProjectSet,
        "library" => Source::Library,
        _ => Source::User,
    }
}

pub(crate) fn parse_source_opt(s: Option<String>) -> Option<Source> {
    s.as_deref().map(parse_source)
}

fn error_code(e: &ActivationError) -> (ActivationErrorCode, String) {
    match e {
        ActivationError::FileMissing(p) => (ActivationErrorCode::FileMissing, format!("{} is missing", p.display())),
        ActivationError::InUse(p) => (ActivationErrorCode::InUse, format!("{} is in use by another app", p.display())),
        ActivationError::InvalidFont(p) => (ActivationErrorCode::InvalidFont, format!("{} is not a valid font", p.display())),
        ActivationError::Os(m) => (ActivationErrorCode::Os, m.clone()),
    }
}

fn code_str(c: ActivationErrorCode) -> String {
    serde_json::to_value(c).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

fn ok(hash: &str, message: Option<String>) -> ActivationResult {
    ActivationResult { hash: hash.into(), ok: true, error: None, message }
}

fn fail(hash: &str, code: ActivationErrorCode, message: impl Into<String>) -> ActivationResult {
    ActivationResult { hash: hash.into(), ok: false, error: Some(code), message: Some(message.into()) }
}

/// Dotted version strings compared numerically: "2.10" > "2.9".
pub(crate) fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

pub(crate) struct LedgerRow {
    pub hash: String,
    pub path: String,
    pub full_name: String,
    pub source: Source,
}

impl LedgerRow {
    pub fn entry(&self) -> LedgerEntry {
        LedgerEntry { path: PathBuf::from(&self.path), full_name: self.full_name.clone() }
    }
}

/// A library file as far as activation cares.
pub(crate) struct FontFile {
    pub hash: String,
    /// Lowercase PostScript names of all faces (several for a TTC).
    pub ps_names: Vec<String>,
    pub font: FontRef,
    pub full_name: String,
    pub version: String,
    pub family_key: String,
    /// A copy Typeshelf may register; system font folders never count.
    pub path: Option<PathBuf>,
    /// The OS already has this file installed.
    pub in_system: bool,
}

pub(crate) fn font_file(conn: &Connection, hash: &str) -> rusqlite::Result<Option<FontFile>> {
    let mut stmt = conn.prepare_cached(
        "SELECT ps_name, family, style, full_name, version, family_key FROM faces WHERE hash = ?1 ORDER BY idx",
    )?;
    let faces: Vec<(String, String, String, String, String, String)> = stmt
        .query_map([hash], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let Some((ps, family, style, full_name, version, family_key)) = faces.first().cloned() else {
        return Ok(None);
    };
    let mut stmt = conn.prepare_cached(
        "SELECT fo.path, fi.rel_path, fo.managed IS 'system' FROM files fi JOIN folders fo ON fo.id = fi.folder_id
         WHERE fi.hash = ?1 AND fo.status != 'offline' ORDER BY fo.managed IS NOT NULL, fi.id",
    )?;
    let copies: Vec<(PathBuf, bool)> = stmt
        .query_map([hash], |r| Ok((Path::new(&r.get::<_, String>(0)?).join(r.get::<_, String>(1)?), r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let in_system = copies.iter().any(|(_, system)| *system);
    Ok(Some(FontFile {
        hash: hash.into(),
        ps_names: faces.iter().map(|f| f.0.to_lowercase()).collect(),
        font: FontRef { hash: hash.into(), ps_name: ps, family, style },
        full_name,
        version,
        family_key,
        path: copies.into_iter().find(|(p, system)| !system && p.is_file()).map(|(p, _)| p),
        in_system,
    }))
}

pub(crate) fn font_ref(conn: &Connection, hash: &str) -> FontRef {
    conn.query_row("SELECT ps_name, family, style FROM faces WHERE hash = ?1 ORDER BY idx LIMIT 1", [hash], |r| {
        Ok(FontRef { hash: hash.into(), ps_name: r.get(0)?, family: r.get(1)?, style: r.get(2)? })
    })
    .unwrap_or_else(|_| FontRef { hash: hash.into(), ps_name: String::new(), family: String::new(), style: String::new() })
}

#[derive(Default, Serialize, Deserialize)]
struct LastRepair {
    at: Option<String>,
    removed: u32,
    reregistered: u32,
}

impl Engine {
    pub(crate) fn ledger(&self, conn: &Connection) -> rusqlite::Result<Vec<LedgerRow>> {
        let mut stmt = conn.prepare_cached("SELECT hash, path, full_name, source FROM activations ORDER BY hash")?;
        let rows = stmt.query_map([], |r| {
            Ok(LedgerRow { hash: r.get(0)?, path: r.get(1)?, full_name: r.get(2)?, source: parse_source(&r.get::<_, String>(3)?) })
        })?;
        rows.collect()
    }

    fn ledger_row(&self, conn: &Connection, hash: &str) -> rusqlite::Result<Option<LedgerRow>> {
        conn.query_row("SELECT hash, path, full_name, source FROM activations WHERE hash = ?1", [hash], |r| {
            Ok(LedgerRow { hash: r.get(0)?, path: r.get(1)?, full_name: r.get(2)?, source: parse_source(&r.get::<_, String>(3)?) })
        })
        .optional()
    }

    pub(crate) fn activation_failed(&self, conn: &Connection, hash: &str, e: &ActivationError) {
        self.emit(Event::ActivationFailed { font: font_ref(conn, hash), reason: code_str(error_code(e).0) });
    }

    fn is_blocked(&self, conn: &Connection, f: &FontFile) -> rusqlite::Result<bool> {
        let mut stmt = conn.prepare_cached("SELECT hash FROM blocked WHERE ps_name = ?1 COLLATE NOCASE")?;
        for ps in &f.ps_names {
            let rows: Vec<Option<String>> = stmt.query_map([ps], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            if rows.iter().any(|h| h.as_deref().is_none_or(|h| h == f.hash)) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Families (display names) among `hashes` whose license is unknown and that would be turned on.
    fn unlicensed_families(&self, conn: &Connection, hashes: &[String]) -> rusqlite::Result<Vec<String>> {
        let mut names = Vec::new();
        for h in hashes {
            let row: Option<String> = conn
                .query_row(
                    "SELECT f.family FROM faces f LEFT JOIN licenses l ON l.family_key = f.family_key
                     WHERE f.hash = ?1 AND f.idx = (SELECT MIN(idx) FROM faces WHERE hash = ?1)
                       AND COALESCE(l.label, 'unknown') = 'unknown'
                       AND NOT EXISTS (SELECT 1 FROM activations WHERE hash = ?1)",
                    [h],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(n) = row.filter(|n| !names.contains(n)) {
                names.push(n);
            }
        }
        Ok(names)
    }

    /// Turns on `hashes`, at most one file per PostScript name: when the batch has several, the newest
    /// version wins; a single pick (the user chose it) replaces whatever version is active.
    pub(crate) fn activate_hashes(&self, conn: &Connection, hashes: &[String], source: Source) -> Result<Vec<ActivationResult>> {
        let mut results = Vec::new();
        let mut by_ps: BTreeMap<String, Vec<FontFile>> = BTreeMap::new();
        let mut seen = HashSet::new();
        for h in hashes.iter().filter(|h| seen.insert(h.as_str())) {
            match font_file(conn, h)? {
                Some(f) => by_ps.entry(f.ps_names[0].clone()).or_default().push(f),
                None => results.push(fail(h, ActivationErrorCode::FileMissing, "not in the library")),
            }
        }
        let mut changed = false;
        for mut group in by_ps.into_values() {
            group.sort_by(|a, b| {
                version_key(&b.version)
                    .cmp(&version_key(&a.version))
                    .then_with(|| is_otf(b).cmp(&is_otf(a)))
                    .then_with(|| a.hash.cmp(&b.hash))
            });
            let winner = group.remove(0);
            for loser in group {
                let msg = format!("{} {} is used instead", winner.full_name, winner.version);
                results.push(fail(&loser.hash, ActivationErrorCode::NameConflict, msg));
            }
            let (result, did) = self.activate_one(conn, &winner, source)?;
            changed |= did;
            results.push(result);
        }
        if changed {
            self.os.notify_font_change();
        }
        Ok(results)
    }

    fn activate_one(&self, conn: &Connection, f: &FontFile, source: Source) -> Result<(ActivationResult, bool)> {
        if self.ledger_row(conn, &f.hash)?.is_some() {
            return Ok((ok(&f.hash, None), false));
        }
        if self.is_blocked(conn, f)? {
            self.emit(Event::ActivationFailed { font: f.font.clone(), reason: "blocked".into() });
            return Ok((fail(&f.hash, ActivationErrorCode::Blocked, "blocked by your admin"), false));
        }
        let Some(path) = f.path.clone() else {
            if f.in_system {
                return Ok((ok(&f.hash, Some(format!("{} is a system font, always on", f.font.ps_name))), false));
            }
            self.emit(Event::ActivationFailed { font: f.font.clone(), reason: "file_missing".into() });
            return Ok((fail(&f.hash, ActivationErrorCode::FileMissing, "the file is missing or its folder is offline"), false));
        };

        // One active file per PostScript name: turn off any other version first.
        let mut stmt = conn.prepare_cached(
            "SELECT DISTINCT a.hash FROM activations a JOIN faces fa ON fa.hash = a.hash
             WHERE fa.ps_name = ?1 COLLATE NOCASE AND a.hash != ?2",
        )?;
        let mut others = Vec::new();
        for ps in &f.ps_names {
            others.extend(stmt.query_map(params![ps, f.hash], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        for other in others {
            if let Some(row) = self.ledger_row(conn, &other)? {
                let r = self.deactivate_one(conn, &row, source)?;
                if !r.ok {
                    let msg = format!("another version could not be turned off: {}", r.message.unwrap_or_default());
                    return Ok((fail(&f.hash, ActivationErrorCode::NameConflict, msg), true));
                }
            }
        }

        let entry = LedgerEntry { path: path.clone(), full_name: f.full_name.clone() };
        if let Err(e) = self.os.activate(&entry) {
            self.activation_failed(conn, &f.hash, &e);
            let (code, msg) = error_code(&e);
            return Ok((fail(&f.hash, code, msg), false));
        }
        conn.execute(
            "INSERT INTO activations (hash, path, full_name, source, activated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![f.hash, path.to_string_lossy(), f.full_name, source_str(source), store::now()],
        )?;
        if matches!(source, Source::User | Source::ProjectSet) {
            add_recent(conn, &f.family_key)?;
        }
        self.emit(Event::FontActivated { font: f.font.clone(), source });
        let warning = f
            .ps_names
            .iter()
            .any(|p| self.system_names().contains(p))
            .then(|| format!("{} is also installed in the system", f.font.ps_name));
        Ok((ok(&f.hash, warning), true))
    }

    fn deactivate_one(&self, conn: &Connection, row: &LedgerRow, source: Source) -> Result<ActivationResult> {
        if let Err(e) = self.os.deactivate(&row.entry()) {
            self.activation_failed(conn, &row.hash, &e);
            let (code, msg) = error_code(&e);
            return Ok(fail(&row.hash, code, msg));
        }
        conn.execute("DELETE FROM activations WHERE hash = ?1", [&row.hash])?;
        let was_admin = row.source == Source::Admin && source != Source::Admin;
        self.emit(Event::FontDeactivated { font: font_ref(conn, &row.hash), source, was_admin });
        Ok(ok(&row.hash, None))
    }

    pub(crate) fn deactivate_hashes(&self, conn: &Connection, hashes: &[String], source: Source) -> Result<Vec<ActivationResult>> {
        let mut results = Vec::new();
        let mut changed = false;
        for h in hashes {
            match self.ledger_row(conn, h)? {
                Some(row) => {
                    let r = self.deactivate_one(conn, &row, source)?;
                    changed |= r.ok;
                    results.push(r);
                }
                None => results.push(ok(h, None)),
            }
        }
        if changed {
            self.os.notify_font_change();
        }
        Ok(results)
    }

    pub fn set_active(&self, hashes: &[String], active: bool, confirm_unknown_license: bool) -> Result<Vec<ActivationResult>> {
        let results = {
            let db = self.db();
            if active {
                if !confirm_unknown_license {
                    let families = self.unlicensed_families(&db, hashes)?;
                    if !families.is_empty() {
                        return Err(unknown_license(families));
                    }
                }
                self.activate_hashes(&db, hashes, Source::User)?
            } else {
                self.deactivate_hashes(&db, hashes, Source::User)?
            }
        };
        self.library_changed("activation");
        Ok(results)
    }

    /// Turning a set off keeps fonts that another active set or an admin still needs.
    pub fn toggle_project_set(&self, id: i64, on: bool, confirm_unknown_license: bool) -> Result<Vec<ActivationResult>> {
        let results = {
            let db = self.db();
            let name: String = db
                .query_row("SELECT name FROM project_sets WHERE id = ?1", [id], |r| r.get(0))
                .optional()?
                .ok_or_else(|| crate::not_found("project set"))?;
            let keys = set_keys(&db, id)?;
            let results = if on {
                let mut hashes = Vec::new();
                let mut stmt = db.prepare(
                    "SELECT DISTINCT f.hash FROM faces f WHERE f.family_key = ?1 AND f.idx = 0
                       AND EXISTS (SELECT 1 FROM files WHERE hash = f.hash)
                       AND NOT EXISTS (SELECT 1 FROM activations a JOIN faces g ON g.hash = a.hash
                                       WHERE g.ps_name = f.ps_name COLLATE NOCASE)",
                )?;
                for k in &keys {
                    hashes.extend(stmt.query_map([k], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?);
                }
                drop(stmt);
                if !confirm_unknown_license {
                    let families = self.unlicensed_families(&db, &hashes)?;
                    if !families.is_empty() {
                        return Err(unknown_license(families));
                    }
                }
                self.activate_hashes(&db, &hashes, Source::ProjectSet)?
            } else {
                let mut stmt = db.prepare(
                    "SELECT DISTINCT i.family_key FROM project_set_items i JOIN project_sets s ON s.id = i.set_id
                     WHERE s.is_on = 1 AND s.id != ?1",
                )?;
                let needed: HashSet<String> = stmt.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                let mut stmt = db.prepare(
                    "SELECT DISTINCT a.hash FROM activations a JOIN faces f ON f.hash = a.hash
                     WHERE f.family_key = ?1 AND a.source NOT IN ('admin', 'library')",
                )?;
                let mut hashes = Vec::new();
                for k in keys.iter().filter(|k| !needed.contains(*k)) {
                    hashes.extend(stmt.query_map([k], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?);
                }
                drop(stmt);
                self.deactivate_hashes(&db, &hashes, Source::ProjectSet)?
            };
            db.execute(
                "UPDATE project_sets SET is_on = ?2, last_used = ?3 WHERE id = ?1",
                params![id, on, store::now()],
            )?;
            let fonts = results.iter().filter(|r| r.ok).count() as u32;
            self.emit(Event::ProjectSetToggled { name, on, fonts });
            results
        };
        self.library_changed("activation");
        Ok(results)
    }

    pub fn health(&self) -> Result<Health> {
        let db = self.db();
        let last: LastRepair = store::get(&db, "last_repair")?.unwrap_or_default();
        let ledger = self.ledger(&db)?;
        let problems = ledger
            .iter()
            .filter(|a| !Path::new(&a.path).is_file())
            .map(|a| Problem { hash: a.hash.clone(), message: format!("file is missing: {}", a.path) })
            .collect();
        Ok(Health {
            last_check: last.at,
            active: ledger.len() as u32,
            problems,
            removed: last.removed,
            reregistered: last.reregistered,
        })
    }

    /// Makes the OS match the ledger. Missing files stay in the ledger so they come back with their
    /// folder; they are reported as problems.
    pub fn repair_now(&self) -> Result<Health> {
        {
            let db = self.db();
            let entries: Vec<LedgerEntry> = self.ledger(&db)?.iter().map(LedgerRow::entry).collect();
            let report = self.os.reconcile(&entries);
            let last = LastRepair {
                at: Some(store::now()),
                removed: report.removed.len() as u32,
                reregistered: report.reregistered.len() as u32,
            };
            store::set(&db, "last_repair", &last)?;
            if last.removed + last.reregistered > 0 {
                self.os.notify_font_change();
            }
            self.emit(Event::RepairRan { removed: last.removed, reregistered: last.reregistered });
        }
        self.health()
    }

    /// Every active font, for `PUT /me/fonts` after startup reconcile.
    pub fn snapshot(&self) -> Result<FontSnapshot> {
        let db = self.db();
        let active = self
            .ledger(&db)?
            .into_iter()
            .map(|a| ActiveFont { font: font_ref(&db, &a.hash), source: a.source })
            .collect();
        Ok(FontSnapshot { active })
    }

    /// Turns everything off (Settings › "Deactivate all on quit").
    pub fn deactivate_all(&self) -> Result<Vec<ActivationResult>> {
        let results = {
            let db = self.db();
            let hashes: Vec<String> = self.ledger(&db)?.into_iter().map(|a| a.hash).collect();
            self.deactivate_hashes(&db, &hashes, Source::User)?
        };
        self.library_changed("activation");
        Ok(results)
    }

    pub(crate) fn admin_label(&self, conn: &Connection, hashes: &[String]) -> Vec<crate::FontLabel> {
        hashes
            .iter()
            .map(|h| font_ref(conn, h))
            .map(|f| crate::FontLabel { family: f.family, style: f.style })
            .collect()
    }

    pub(crate) fn admin_change(&self, action: &'static str, fonts: Vec<crate::FontLabel>) {
        self.ui(UiEvent::AdminChange { action, fonts });
        self.library_changed("admin");
    }
}

fn is_otf(f: &FontFile) -> bool {
    f.path.as_ref().and_then(|p| p.extension()).is_some_and(|e| e.eq_ignore_ascii_case("otf"))
}

fn unknown_license(families: Vec<String>) -> EngineError {
    EngineError::new("unknown_license", format!("{} has no license label", families.join(", "))).with("families", families)
}

pub(crate) fn set_keys(conn: &Connection, id: i64) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached("SELECT family_key FROM project_set_items WHERE set_id = ?1 ORDER BY family_key")?;
    let keys = stmt.query_map([id], |r| r.get(0))?.collect();
    keys
}

fn add_recent(conn: &Connection, family_key: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO collection_items (collection_id, family_key, added_at)
         SELECT id, ?1, ?2 FROM collections WHERE builtin = 'recent'
         ON CONFLICT (collection_id, family_key) DO UPDATE SET added_at = excluded.added_at",
        params![family_key, store::now()],
    )?;
    conn.execute(
        "DELETE FROM collection_items WHERE collection_id = (SELECT id FROM collections WHERE builtin = 'recent')
         AND family_key NOT IN (SELECT family_key FROM collection_items
             WHERE collection_id = (SELECT id FROM collections WHERE builtin = 'recent')
             ORDER BY added_at DESC LIMIT 30)",
        [],
    )?;
    Ok(())
}
