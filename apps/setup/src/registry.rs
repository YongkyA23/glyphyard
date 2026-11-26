//! HKCU entries: the Add/Remove Programs entry, the login item, and font registrations.

use std::io;
use std::path::{Path, PathBuf};

use windows_registry::CURRENT_USER;

use crate::process::APP_EXE;

const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Typeshelf";
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const FONTS: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Fonts";
/// The value name tauri-plugin-autostart uses (the app's productName).
const LOGIN_ITEM: &str = "Typeshelf";

pub struct Installed {
    pub dir: PathBuf,
    pub version: String,
}

pub fn installed() -> Option<Installed> {
    let key = CURRENT_USER.open(UNINSTALL).ok()?;
    Some(Installed {
        dir: key.get_string("InstallLocation").ok().filter(|d| !d.is_empty())?.into(),
        version: key.get_string("DisplayVersion").unwrap_or_default(),
    })
}

pub fn write_uninstall(dir: &Path, version: &str, size_kb: u32) -> io::Result<()> {
    let key = CURRENT_USER.create(UNINSTALL)?;
    let uninstaller = dir.join("uninstall.exe");
    let u = uninstaller.display();
    let strings = [
        ("DisplayName", "Glyphyard".to_string()),
        ("DisplayVersion", version.to_string()),
        ("Publisher", "Glyphyard".to_string()),
        ("DisplayIcon", dir.join(APP_EXE).display().to_string()),
        ("InstallLocation", dir.display().to_string()),
        ("UninstallString", format!("\"{u}\" --uninstall")),
        ("QuietUninstallString", format!("\"{u}\" --uninstall --silent")),
    ];
    for (name, value) in strings {
        key.set_string(name, value)?;
    }
    key.set_u32("EstimatedSize", size_kb)?;
    key.set_u32("NoModify", 1)?;
    key.set_u32("NoRepair", 1)?;
    Ok(())
}

pub fn remove_uninstall() -> io::Result<()> {
    if CURRENT_USER.open(UNINSTALL).is_ok() {
        CURRENT_USER.remove_tree(UNINSTALL)?;
    }
    Ok(())
}

pub fn remove_login_item() {
    for path in [RUN, STARTUP_APPROVED] {
        if let Ok(key) = CURRENT_USER.options().read().write().open(path) {
            let _ = key.remove_value(LOGIN_ITEM);
        }
    }
}

/// Font registrations made by Glyphyard: HKCU Fonts values named "<font> (Typeshelf)" (the pre-rename name).
pub fn typeshelf_fonts() -> usize {
    let Ok(key) = CURRENT_USER.open(FONTS) else { return 0 };
    key.values().map_or(0, |values| values.filter(|(name, _)| name.ends_with(" (Typeshelf)")).count())
}
