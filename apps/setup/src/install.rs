//! Install and update (same flow: an update installs over the existing folder).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::process::{self, APP_EXE};
use crate::shortcuts::{self, Place};
use crate::{log, payload, registry, Error, VERSION};

pub struct Options {
    pub dir: PathBuf,
    pub start_menu: bool,
    pub desktop: bool,
    pub start_at_login: bool,
}

pub fn default_dir() -> PathBuf {
    registry::installed().map(|i| i.dir).unwrap_or_else(|| env_dir("LOCALAPPDATA").join(r"Programs\Typeshelf"))
}

/// Payload plus uninstall.exe (a copy of this program), in bytes.
pub fn total_size() -> u64 {
    let own = std::env::current_exe().and_then(fs::metadata).map_or(0, |m| m.len());
    payload::size() + own
}

pub fn install(o: &Options, progress: &dyn Fn(u32, &str)) -> Result<(), Error> {
    if process::app_running() {
        return Err(Error::app_running());
    }
    validate_dir(&o.dir)?;
    // An update keeps the user's earlier choices: the UI sends default options in update mode, and the
    // app's own "start at login" setting stays authoritative.
    let update = registry::installed().is_some();
    let start_menu = if update { shortcuts::exists(Place::StartMenu) } else { o.start_menu };
    let desktop = if update { shortcuts::exists(Place::Desktop) } else { o.desktop };
    log(format!("{} {VERSION} into {}", if update { "updating to" } else { "installing" }, o.dir.display()));

    // Plug-in packages from an older version must not linger next to the new ones.
    match fs::remove_dir_all(o.dir.join("plugins")) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    payload::extract(&o.dir, |rel, done, total| {
        let step = if rel.starts_with("plugins") { "plugins" } else { "copy" };
        progress((done * 80 / total) as u32, step);
    })?;
    let me = std::env::current_exe()?;
    let uninstaller = o.dir.join("uninstall.exe");
    if !same_file(&me, &uninstaller) {
        fs::copy(&me, &uninstaller)?;
    }

    progress(85, "webview2");
    match webview2_version() {
        Some(v) => log(format!("WebView2 {v}")),
        None => log("WebView2 runtime is missing: Typeshelf won't open until it is installed (https://go.microsoft.com/fwlink/p/?LinkId=2124703)"),
    }

    progress(90, "shortcuts");
    let app = o.dir.join(APP_EXE);
    shortcuts::set(Place::StartMenu, &app, start_menu)?;
    shortcuts::set(Place::Desktop, &app, desktop)?;

    progress(95, "registry");
    if !update {
        process::run_app(&o.dir, &format!("--set-start-at-login={}", o.start_at_login))?;
    }
    registry::write_uninstall(&o.dir, VERSION, (total_size() / 1024) as u32)?;
    progress(100, "registry");
    log("install finished");
    Ok(())
}

pub fn webview2_version() -> Option<String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::GetAvailableCoreWebView2BrowserVersionString;
    let mut version = windows::core::PWSTR::null();
    unsafe { GetAvailableCoreWebView2BrowserVersionString(windows::core::PCWSTR::null(), &mut version) }.ok()?;
    if version.is_null() {
        return None;
    }
    Some(webview2_com::take_pwstr(version)).filter(|v| !v.is_empty())
}

/// Absolute, not a system/profile folder, empty or an existing Typeshelf install (uninstall deletes the
/// whole folder), and writable.
pub fn validate_dir(dir: &Path) -> Result<(), Error> {
    let invalid = |msg: &str| Err(Error::new("invalid_dir", msg));
    if !dir.is_absolute() {
        return invalid("Choose a full folder path, like C:\\Users\\you\\AppData\\Local\\Programs\\Typeshelf.");
    }
    if is_protected(dir) {
        return invalid("Typeshelf can't be installed directly in a system or user folder. Choose a folder of its own.");
    }
    if let Ok(mut entries) = fs::read_dir(dir) {
        if entries.next().is_some() && !looks_installed(dir) {
            return invalid("That folder isn't empty. Choose an empty folder or a new one.");
        }
    }
    let probe = dir.join(".typeshelf-write-test");
    let writable = fs::create_dir_all(dir).and_then(|_| fs::write(&probe, b"")).and_then(|_| fs::remove_file(&probe));
    if let Err(e) = writable {
        return invalid(&format!("Setup can't write to that folder ({e}). Choose another one."));
    }
    Ok(())
}

pub fn looks_installed(dir: &Path) -> bool {
    dir.join(APP_EXE).is_file() || dir.join("uninstall.exe").is_file()
}

/// Drive roots, Windows and Program Files (and anything inside Windows), and the profile folders themselves.
pub fn is_protected(dir: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
    let d = norm(dir);
    if dir.parent().is_none() || !d.contains('\\') {
        return true;
    }
    let windows = norm(&env_dir("WINDIR"));
    if d == windows || d.starts_with(&format!("{windows}\\")) {
        return true;
    }
    let exact = [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "ProgramData",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
    ];
    exact.iter().map(|v| env_dir(v)).chain([env_dir("LOCALAPPDATA").join("Programs")]).any(|p| norm(&p) == d)
}

pub fn env_dir(name: &str) -> PathBuf {
    std::env::var_os(name).map(PathBuf::from).unwrap_or_default()
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protects_system_and_profile_folders() {
        assert!(is_protected(Path::new(r"C:\")));
        assert!(is_protected(&env_dir("WINDIR").join("System32")));
        assert!(is_protected(&env_dir("USERPROFILE")));
        assert!(is_protected(&env_dir("LOCALAPPDATA").join("Programs\\")));
        assert!(!is_protected(&env_dir("LOCALAPPDATA").join(r"Programs\Typeshelf")));
    }

    #[test]
    fn rejects_foreign_non_empty_folders() {
        let dir = env_dir("TEMP").join(format!("ts-validate-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("notes.txt"), "x").unwrap();
        assert_eq!(validate_dir(&dir).unwrap_err().code, "invalid_dir");
        fs::write(dir.join(APP_EXE), "x").unwrap();
        assert!(validate_dir(&dir).is_ok());
        fs::remove_dir_all(&dir).unwrap();
    }
}
