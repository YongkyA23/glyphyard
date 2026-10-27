//! Commands pushed by the admin server.

use crate::activation::version_key;
use crate::library::set_license_row;
use crate::{EngineError, Engine, License, Result, UiEvent};
use rusqlite::{params, Connection};
use typeshelf_core::file_hash;
use typeshelf_core::organize::sanitize;
use typeshelf_protocol::{CommandKind, FontRef, LibraryFile, Source};

/// Local file for an admin font: the exact hash, else the newest version with that PostScript name.
fn resolve(conn: &Connection, font: &FontRef) -> rusqlite::Result<Option<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT f.hash, f.version FROM faces f WHERE (f.hash = ?1 OR f.ps_name = ?2 COLLATE NOCASE)
           AND EXISTS (SELECT 1 FROM files WHERE hash = f.hash)",
    )?;
    let mut rows: Vec<(String, String)> =
        stmt.query_map(params![font.hash, font.ps_name], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    if let Some(exact) = rows.iter().find(|(h, _)| *h == font.hash) {
        return Ok(Some(exact.0.clone()));
    }
    rows.sort_by_key(|(_, v)| std::cmp::Reverse(version_key(v)));
    Ok(rows.into_iter().next().map(|(h, _)| h))
}

/// Active hashes matching an admin font reference (exact hash or same PostScript name).
fn active_matching(conn: &Connection, hash: Option<&str>, ps_name: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT a.hash FROM activations a LEFT JOIN faces f ON f.hash = a.hash
         WHERE a.hash = ?1 OR f.ps_name = ?2 COLLATE NOCASE",
    )?;
    let rows = stmt.query_map(params![hash, ps_name], |r| r.get(0))?;
    rows.collect()
}

fn first_error(results: &[crate::ActivationResult]) -> Result<()> {
    match results.iter().find(|r| !r.ok) {
        Some(r) => {
            let code = serde_json::to_value(r.error).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
            Err(EngineError::new(&code, r.message.clone().unwrap_or_default()))
        }
        None => Ok(()),
    }
}

impl Engine {
    /// Applies a pushed command; the result is what the sync client acks. `Deliver` of a file not yet
    /// in the library fails with `download_required`: download it and call [`Engine::deliver`].
    pub fn apply_command(&self, cmd: &CommandKind) -> Result<()> {
        match cmd {
            CommandKind::Activate { font } => {
                let (results, labels) = {
                    let db = self.db();
                    let Some(hash) = resolve(&db, font)? else {
                        self.emit(typeshelf_protocol::Event::ActivationFailed { font: font.clone(), reason: "file_missing".into() });
                        return Err(EngineError::new("file_missing", format!("{} is not in this library", font.ps_name)));
                    };
                    let results = self.activate_hashes(&db, std::slice::from_ref(&hash), Source::Admin)?;
                    (results, self.admin_label(&db, &[hash]))
                };
                self.admin_change("activated", labels);
                first_error(&results)
            }
            CommandKind::Deactivate { font } => {
                let (results, labels) = {
                    let db = self.db();
                    let hashes = active_matching(&db, Some(&font.hash), &font.ps_name)?;
                    let labels = self.admin_label(&db, &hashes);
                    (self.deactivate_hashes(&db, &hashes, Source::Admin)?, labels)
                };
                self.admin_change("deactivated", labels);
                first_error(&results)
            }
            CommandKind::Block { ps_name, hash } => {
                let (results, labels) = {
                    let db = self.db();
                    db.execute(
                        "INSERT INTO blocked (ps_name, hash) VALUES (?1, ?2) ON CONFLICT (ps_name) DO UPDATE SET hash = excluded.hash",
                        params![ps_name, hash],
                    )?;
                    let mut hashes = active_matching(&db, hash.as_deref(), ps_name)?;
                    if let Some(h) = hash {
                        // Only that file is blocked; other files with the name stay.
                        hashes.retain(|x| x == h);
                    }
                    let labels = self.admin_label(&db, &hashes);
                    (self.deactivate_hashes(&db, &hashes, Source::Admin)?, labels)
                };
                self.admin_change("blocked", labels);
                first_error(&results)
            }
            CommandKind::Unblock { ps_name } => {
                self.db().execute("DELETE FROM blocked WHERE ps_name = ?1", [ps_name])?;
                Ok(())
            }
            CommandKind::Deliver { file, activate } => {
                let known: bool = self.db().query_row("SELECT EXISTS (SELECT 1 FROM files WHERE hash = ?1)", [&file.hash], |r| r.get(0))?;
                if !known {
                    return Err(EngineError::new("download_required", format!("{} must be downloaded", file.filename)));
                }
                self.finish_delivery(file, *activate)
            }
            CommandKind::OrganizeRequest => {
                self.ui(UiEvent::OrganizeRequested {});
                Ok(())
            }
            CommandKind::PolicyChanged { policy } => self.set_policy(policy),
        }
    }

    /// Saves a shared-library file into the managed "Shared library" folder, optionally turning it on.
    pub fn deliver(&self, file: &LibraryFile, bytes: &[u8], activate: bool) -> Result<()> {
        if file_hash(bytes) != file.hash {
            return Err(EngineError::new("hash_mismatch", format!("{} did not match its hash", file.filename)));
        }
        let (id, dir) = self.managed_folder("shared", "Shared library")?;
        let mut name = sanitize(&file.filename);
        if name.is_empty() || name.starts_with('.') {
            name = format!("{}.otf", &file.hash[..8]);
        }
        let mut path = dir.join(&name);
        if path.exists() && std::fs::read(&path).map(|b| file_hash(&b)).ok().as_deref() != Some(file.hash.as_str()) {
            path = dir.join(format!("{}-{name}", &file.hash[..8]));
        }
        if !path.exists() {
            std::fs::write(&path, bytes)?;
        }
        self.scan_folder(id)?;
        self.finish_delivery(file, activate)
    }

    fn finish_delivery(&self, file: &LibraryFile, activate: bool) -> Result<()> {
        let (results, labels) = {
            let db = self.db();
            let license = License::parse(&file.license.to_lowercase());
            if license != License::Unknown {
                let keys: Vec<String> = {
                    let mut stmt = db.prepare("SELECT DISTINCT family_key FROM faces WHERE hash = ?1")?;
                    let keys = stmt.query_map([&file.hash], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                    keys
                };
                for k in keys {
                    set_license_row(&db, &k, license, None, "from the shared library")?;
                }
            }
            let hashes = [file.hash.clone()];
            let results = if activate { self.activate_hashes(&db, &hashes, Source::Library)? } else { vec![] };
            (results, self.admin_label(&db, &hashes))
        };
        self.admin_change("delivered", labels);
        first_error(&results)
    }
}
