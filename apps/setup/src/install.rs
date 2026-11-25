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

    // Extract next to the current install and swap only a complete payload in: a failure before the swap
    // leaves the previous version untouched, and swap_in puts it back if the swap itself fails.
    let staging = o.dir.join(STAGING);
    remove_if_present(&staging)?;
    let extracted = payload::extract(&staging, |rel, done, total| {
        let step = if rel.starts_with("plugins") { "plugins" } else { "copy" };
        progress((done * 80 / total) as u32, step);
    });
    let extracted = extracted.and_then(|_| match staging.join(APP_EXE).is_file() {
        true => Ok(()),
        false => Err(io::Error::other(format!("this Setup contains no {APP_EXE}"))),
    });
    if let Err(e) = extracted {
        let _ = remove_if_present(&staging);
        return Err(e.into());
    }
    swap_in(&o.dir)?;
    let me = std::env::current_exe()?;
    let uninstaller = o.dir.join("uninstall.exe");
    if !same_file(&me, &uninstaller) {
        fs::copy(&me, &uninstaller)?;
    }
    // Registered as soon as the files are in place, so a failure below still leaves it removable.
    registry::write_uninstall(&o.dir, VERSION, (total_size() / 1024) as u32)?;

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
    if let Ok(entries) = fs::read_dir(dir) {
        // A killed install leaves STAGING/BACKUP, and mid-swap plugins/ next to them: ours to replace.
        let names: Vec<_> = entries.map(|e| e.map(|e| e.file_name()).unwrap_or_default()).collect();
        let ours = |n: &std::ffi::OsStr| n == STAGING || n == BACKUP;
        let marked = names.iter().any(|n| ours(n));
        let foreign = names.iter().any(|n| !ours(n) && !(marked && n.eq_ignore_ascii_case("plugins")));
        if foreign && !looks_installed(dir) {
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

/// Payload extracted here first; swapped in only once it is complete.
const STAGING: &str = ".setup-staging";
/// The previous version's files, kept until the swap is complete.
const BACKUP: &str = ".setup-previous";
/// What an update replaces (the previous plug-in packages must not linger next to the new ones).
const SWAPPED: [&str; 2] = ["plugins", APP_EXE];

/// Moves the staged files into `dir`, the current ones into BACKUP first. On failure puts the current ones
/// back. Either way STAGING and BACKUP are gone afterwards (best effort: leftovers are only logged).
fn swap_in(dir: &Path) -> io::Result<()> {
    let (staging, backup) = (dir.join(STAGING), dir.join(BACKUP));
    remove_if_present(&backup)?;
    fs::create_dir(&backup)?;
    // Each entry: (name, old one moved aside, new one moved in).
    let mut done: Vec<(&str, bool, bool)> = Vec::new();
    let mut result = Ok(());
    for name in SWAPPED {
        let step = done.len();
        done.push((name, false, false));
        let moved = (|| {
            if fs::symlink_metadata(dir.join(name)).is_ok() {
                fs::rename(dir.join(name), backup.join(name))?;
                done[step].1 = true;
            }
            if fs::symlink_metadata(staging.join(name)).is_ok() {
                fs::rename(staging.join(name), dir.join(name))?;
                done[step].2 = true;
            }
            io::Result::Ok(())
        })();
        if let Err(e) = moved {
            result = Err(e);
            break;
        }
    }
    if let Err(e) = &result {
        log(format!("swapping in the new files failed ({e}), putting the previous ones back"));
        for &(name, old_aside, new_in) in done.iter().rev() {
            let back = (|| {
                if new_in {
                    remove_if_present(&dir.join(name))?;
                }
                if old_aside {
                    fs::rename(backup.join(name), dir.join(name))?;
                }
                io::Result::Ok(())
            })();
            if let Err(e) = back {
                log(format!("couldn't restore {name} (the previous version is in {}): {e}", backup.display()));
                return result; // keep BACKUP: it holds the only copy
            }
        }
    }
    for leftover in [&staging, &backup] {
        if let Err(e) = remove_if_present(leftover) {
            log(format!("couldn't remove {}: {e}", leftover.display()));
        }
    }
    result
}

pub fn remove_if_present(path: &Path) -> io::Result<()> {
    let removed = match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) => Err(e),
    };
    match removed {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
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

    #[test]
    fn accepts_a_folder_holding_only_setups_leftovers() {
        let dir = env_dir("TEMP").join(format!("ts-validate-leftover-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // Killed during extraction.
        fs::create_dir_all(dir.join(STAGING)).unwrap();
        assert!(validate_dir(&dir).is_ok());
        // Killed mid-swap on a fresh install: plugins/ already moved in, no typeshelf.exe yet.
        fs::create_dir_all(dir.join(BACKUP)).unwrap();
        fs::create_dir_all(dir.join("plugins")).unwrap();
        assert!(validate_dir(&dir).is_ok());
        // Anything else is still someone's.
        fs::write(dir.join("notes.txt"), "x").unwrap();
        assert_eq!(validate_dir(&dir).unwrap_err().code, "invalid_dir");
        fs::remove_dir_all(&dir).unwrap();
        // plugins/ alone is not marked as ours.
        fs::create_dir_all(dir.join("plugins")).unwrap();
        assert_eq!(validate_dir(&dir).unwrap_err().code, "invalid_dir");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// An install folder holding `version` in typeshelf.exe and plugins/p.zxp, plus a staged `next`.
    fn installed_with_staged(name: &str, version: &str, next: &str) -> PathBuf {
        let dir = env_dir("TEMP").join(format!("ts-swap-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for (root, v) in [(dir.clone(), version), (dir.join(STAGING), next)] {
            fs::create_dir_all(root.join("plugins")).unwrap();
            fs::write(root.join(APP_EXE), v).unwrap();
            fs::write(root.join(r"plugins\p.zxp"), v).unwrap();
        }
        fs::write(dir.join("uninstall.exe"), "u").unwrap();
        dir
    }

    fn read(path: PathBuf) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn swap_replaces_files_and_cleans_up() {
        let dir = installed_with_staged("ok", "old", "new");
        swap_in(&dir).unwrap();
        assert_eq!(read(dir.join(APP_EXE)), "new");
        assert_eq!(read(dir.join(r"plugins\p.zxp")), "new");
        assert!(!dir.join(STAGING).exists() && !dir.join(BACKUP).exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failed_swap_restores_the_previous_files() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = installed_with_staged("fail", "old", "new");
        // No sharing: the staged exe can't be moved, so the swap fails after plugins/ went in.
        let lock = fs::OpenOptions::new().read(true).share_mode(0).open(dir.join(STAGING).join(APP_EXE)).unwrap();
        assert!(swap_in(&dir).is_err());
        drop(lock);
        assert_eq!(read(dir.join(APP_EXE)), "old");
        assert_eq!(read(dir.join(r"plugins\p.zxp")), "old");
        assert!(!dir.join(BACKUP).exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
