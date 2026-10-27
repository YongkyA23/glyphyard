//! Uninstall: turn fonts off, remove plug-ins, shortcuts, registry entries and files.

use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;
use typeshelf_adobe_bridge::install::{detect_apps, remove};

use crate::install::{env_dir, is_protected, looks_installed};
use crate::process::{self, APP_EXE};
use crate::shortcuts::{self, Place};
use crate::{log, registry, Error};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub fonts_turned_off: usize,
    pub leftover_entries: usize,
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
    if wipe_data {
        for root in ["APPDATA", "LOCALAPPDATA"] {
            match fs::remove_dir_all(env_dir(root).join(DATA_DIR)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
    }
    progress(100, "files");
    let report = Report { fonts_turned_off: before.saturating_sub(leftover), leftover_entries: leftover };
    log(format!("uninstall finished: {} fonts turned off, {leftover} left", report.fonts_turned_off));
    Ok(report)
}

/// The uninstall.exe that relaunched us from %TEMP% may still be exiting, so retry for a few seconds.
fn remove_dir_retrying(dir: &Path) -> io::Result<()> {
    for _ in 0..20 {
        match fs::remove_dir_all(dir) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(500)),
            Ok(()) => return Ok(()),
        }
    }
    fs::remove_dir_all(dir)
}
