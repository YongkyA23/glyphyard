//! Register fonts with the OS for the current user and keep the OS in sync with our ledger.
//! Windows: AddFontResourceW + HKCU Fonts value "<full name> (Typeshelf)" + WM_FONTCHANGE.
//! macOS: CTFontManagerRegisterFontsForURL, user scope.
//! This file is the API contract; implementations live in `windows.rs` / `macos.rs`.

use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

#[cfg(windows)]
#[path = "windows.rs"]
mod sys;

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod sys;

#[cfg(not(any(windows, target_os = "macos")))]
mod sys {
    use super::*;
    pub fn activate(_: &LedgerEntry) -> Result<(), ActivationError> {
        Err(ActivationError::Os("unsupported".into()))
    }
    pub fn deactivate(_: &LedgerEntry) -> Result<(), ActivationError> {
        Err(ActivationError::Os("unsupported".into()))
    }
    pub fn reconcile(_: &[LedgerEntry]) -> ReconcileReport {
        ReconcileReport::default()
    }
    pub fn installed_ps_names() -> HashSet<String> {
        HashSet::new()
    }
    pub fn notify_font_change() {}
}

/// A font file we believe is active.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LedgerEntry {
    pub path: PathBuf,
    /// Full font name, used for the Windows registry value name.
    pub full_name: String,
}

#[derive(Debug)]
pub enum ActivationError {
    FileMissing(PathBuf),
    /// Another app holds the file (Windows) — usually a design app with the font open.
    InUse(PathBuf),
    InvalidFont(PathBuf),
    Os(String),
}

impl fmt::Display for ActivationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileMissing(p) => write!(f, "file missing: {}", p.display()),
            Self::InUse(p) => write!(f, "file in use by another app: {}", p.display()),
            Self::InvalidFont(p) => write!(f, "not a valid font: {}", p.display()),
            Self::Os(e) => write!(f, "OS error: {e}"),
        }
    }
}

impl std::error::Error for ActivationError {}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ReconcileReport {
    /// Our OS registrations removed because they were not in the ledger or their file was gone.
    pub removed: Vec<String>,
    /// Ledger entries the OS had lost and we registered again.
    pub reregistered: Vec<PathBuf>,
    /// Ledger entries whose file no longer exists (caller should mark them inactive).
    pub missing: Vec<PathBuf>,
}

/// Registers the file for the current user (persists across reboots). Does not broadcast; call
/// [`notify_font_change`] once after a batch.
pub fn activate(entry: &LedgerEntry) -> Result<(), ActivationError> {
    sys::activate(entry)
}

/// Unregisters the file. Succeeds if it was not registered.
pub fn deactivate(entry: &LedgerEntry) -> Result<(), ActivationError> {
    sys::deactivate(entry)
}

/// Makes the OS match the ledger. Never touches registrations made by other apps.
/// Broadcasts the font change itself when it changed anything.
pub fn reconcile(ledger: &[LedgerEntry]) -> ReconcileReport {
    sys::reconcile(ledger)
}

/// Lowercase PostScript names of fonts installed outside Typeshelf (system and per-user font
/// folders), used to warn about conflicts.
pub fn installed_ps_names() -> HashSet<String> {
    sys::installed_ps_names()
}

/// Tells running apps the font list changed.
pub fn notify_font_change() {
    sys::notify_font_change()
}

/// Appends every file under `dir` (recursively) to `out`. Unreadable entries are skipped.
#[cfg(any(windows, target_os = "macos"))]
fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&e.path(), out),
            Ok(t) if t.is_file() => out.push(e.path()),
            _ => {}
        }
    }
}

/// Lowercase PostScript names of every face in `files`, parsed in parallel. Unreadable or
/// non-font files are skipped.
#[cfg(any(windows, target_os = "macos"))]
fn ps_names_of(files: &[PathBuf]) -> HashSet<String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut names = Vec::new();
                    while let Some(path) = files.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let Ok(bytes) = std::fs::read(path) else { continue };
                        let Ok(faces) = typeshelf_core::parse_faces(&bytes) else { continue };
                        names.extend(faces.into_iter().map(|f| f.ps_name.to_lowercase()));
                    }
                    names
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap_or_default())
            .filter(|n| !n.is_empty())
            .collect()
    })
}
