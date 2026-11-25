//! Uninstall: turn fonts off, remove plug-ins, shortcuts, registry entries and files.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use typeshelf_adobe_bridge::install::{detect_apps, remove};

use crate::install::{env_dir, is_protected, looks_installed, remove_if_present};
use crate::process::{self, APP_EXE};
use crate::shortcuts::{self, Place};
use crate::{log, registry, Error};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub fonts_turned_off: usize,
    pub leftover_entries: usize,
    /// The data folder still holding fonts imported into Typeshelf after "delete my library data".
    pub fonts_kept_in: Option<PathBuf>,
}

/// The desktop app's data folders (Tauri identifier).
const DATA_DIR: &str = "app.typeshelf.desktop";

pub fn uninstall(dir: &Path, wipe_data: bool, progress: &dyn Fn(u32, &str)) -> Result<Report, Error> {
    if process::app_running() {
        return Err(Error::app_running());
    }
    log(format!("uninstalling {} (wipe data: {wipe_data})", dir.display()));

    progress(5, "fonts");
    let before = registry::typeshelf_fonts();
    if dir.join(APP_EXE).is_file() {
        if let Err(e) = process::run_app(dir, "--unregister-fonts") {
            log(format!("turning fonts off: {e}"));
        }
    }
    let leftover = registry::typeshelf_fonts();

    progress(30, "plugins");
    for app in detect_apps().into_iter().filter(|a| a.installed) {
        if let Err(e) = remove(app.app) {
            log(format!("removing the {} plug-in: {e}", app.label));
        }
    }

    progress(55, "shortcuts");
    for place in [Place::StartMenu, Place::Desktop] {
        if let Err(e) = shortcuts::remove(place) {
            log(format!("removing a shortcut: {e}"));
        }
    }

    progress(70, "registry");
    registry::remove_login_item();
    registry::remove_uninstall()?;

    progress(85, "files");
    // Guard against a damaged InstallLocation pointing somewhere we must not delete.
    if looks_installed(dir) && !is_protected(dir) {
        remove_dir_retrying(dir)?;
    } else {
        log(format!("not deleting {}: it doesn't look like a Typeshelf folder", dir.display()));
    }
    let mut fonts_kept_in = None;
    if wipe_data {
        for root in ["APPDATA", "LOCALAPPDATA"] {
            let data = env_dir(root).join(DATA_DIR);
            if wipe_data_dir(&data)? {
                log(format!("kept the font folders in {}", data.display()));
                fonts_kept_in.get_or_insert(data);
            }
        }
    }
    progress(100, "files");
    let report = Report { fonts_turned_off: before.saturating_sub(leftover), leftover_entries: leftover, fonts_kept_in };
    log(format!("uninstall finished: {} fonts turned off, {leftover} left", report.fonts_turned_off));
    Ok(report)
}

/// The desktop app's font folders inside its data folder (`managed_folder` in crates/engine/src/scan.rs).
const FONT_DIRS: [&str; 3] = ["Imported fonts", "Google Fonts", "Shared library"];

/// Deletes a data folder except its font folders that hold at least one file. Returns true when it kept
/// any (and so the folder itself).
fn wipe_data_dir(dir: &Path) -> io::Result<bool> {
    let entries = match fs::read_dir(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        other => other?,
    };
    let mut kept = false;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if FONT_DIRS.iter().any(|n| entry.file_name() == *n) && has_file(&path) {
            kept = true;
        } else if entry.file_type()?.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    if !kept {
        fs::remove_dir(dir)?;
    }
    Ok(kept)
}

/// True when `dir` is a folder with a file anywhere inside it.
fn has_file(dir: &Path) -> bool {
    fs::read_dir(dir).into_iter().flatten().flatten().any(|e| e.file_type().is_ok_and(|t| !t.is_dir() || has_file(&e.path())))
}

/// The uninstall.exe that relaunched us from %TEMP% may still be exiting (or, with --silent, waits for us),
/// so remove everything else, retrying for a few seconds, and if uninstall.exe is still held, delete the
/// folder once we have exited.
fn remove_dir_retrying(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.file_name().is_some_and(|n| n.eq_ignore_ascii_case("uninstall.exe")) {
            continue;
        }
        let mut tries = 0;
        while let Err(e) = remove_if_present(&path) {
            tries += 1;
            if tries == 20 {
                return Err(e);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
    if fs::remove_dir_all(dir).is_err() {
        log(format!("uninstall.exe is still running; {} is deleted once Setup exits", dir.display()));
        process::delete_after_exit(&[dir]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wipe_keeps_font_folders_with_files_only() {
        let dir = env_dir("TEMP").join(format!("ts-wipe-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(r"Imported fonts\Sub")).unwrap();
        fs::write(dir.join(r"Imported fonts\Sub\a.otf"), "x").unwrap();
        fs::create_dir_all(dir.join("Google Fonts")).unwrap();
        fs::create_dir_all(dir.join(r"Shared library\empty")).unwrap();
        fs::create_dir_all(dir.join("logs")).unwrap();
        fs::write(dir.join("library.db"), "x").unwrap();
        assert!(wipe_data_dir(&dir).unwrap());
        let left: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, ["Imported fonts"]);
        assert!(dir.join(r"Imported fonts\Sub\a.otf").is_file());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn wipe_removes_the_folder_without_fonts() {
        let dir = env_dir("TEMP").join(format!("ts-wipe-empty-{}", std::process::id()));
        fs::create_dir_all(dir.join("Google Fonts")).unwrap();
        fs::write(dir.join("library.db"), "x").unwrap();
        assert!(!wipe_data_dir(&dir).unwrap());
        assert!(!dir.exists());
        assert!(!wipe_data_dir(&dir).unwrap(), "a missing folder is fine");
    }
}
