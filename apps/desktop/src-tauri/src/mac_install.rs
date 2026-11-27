//! The macOS install at quit: the downloaded `.app.tar.gz` is verified in memory, unpacked next to the running
//! bundle and swapped into its place in one rename. No relaunch. Compiles everywhere so it is tested on Windows;
//! only the call sites are macOS-only.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use typeshelf_core::update;

use crate::update::{Ready, MOVE};

/// A reason this Mac cannot install updates (not in a writable Applications folder), shown in Settings.
pub fn blocker() -> Option<&'static str> {
    let exe = std::env::current_exe().ok()?;
    match bundle_of(&exe) {
        Some(bundle) if can_replace(&bundle) => None,
        _ => Some(MOVE),
    }
}

/// `<X>.app/Contents/MacOS/<bin>` → `<X>.app`.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let bundle = exe.ancestors().nth(3)?;
    bundle.file_name()?.to_str()?.ends_with(".app").then(|| bundle.to_path_buf())
}

/// Moving the bundle needs write access to it and to the folder holding it (not the DMG, App Translocation, or
/// an /Applications this user can't write).
pub fn can_replace(bundle: &Path) -> bool {
    bundle.parent().is_some_and(|parent| writable(bundle) && writable(parent))
}

#[cfg(unix)]
fn writable(path: &Path) -> bool {
    use std::ffi::{c_char, c_int, CString};
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        fn access(path: *const c_char, mode: c_int) -> c_int;
    }
    const W_OK: c_int = 2;
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else { return false };
    // SAFETY: `path` is a NUL-terminated string that outlives the call.
    unsafe { access(path.as_ptr(), W_OK) == 0 }
}

#[cfg(not(unix))]
fn writable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| !m.permissions().readonly())
}

#[derive(Debug)]
pub enum InstallError {
    /// macOS refused the swap (App Management): the update file is kept for the next quit.
    Blocked(String),
    /// Anything else: the update file is deleted.
    Failed(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            InstallError::Blocked(e) | InstallError::Failed(e) => f.write_str(e),
        }
    }
}

/// Verifies the update, unpacks it beside `bundle` and swaps it into place. The update file is deleted unless
/// macOS blocked the swap; the staging folder is always removed.
pub fn install(ready: &Ready, bundle: &Path) -> Result<(), InstallError> {
    let result = verify_and_swap(ready, bundle);
    if !matches!(result, Err(InstallError::Blocked(_))) {
        let _ = fs::remove_file(&ready.path);
    }
    result
}

fn verify_and_swap(ready: &Ready, bundle: &Path) -> Result<(), InstallError> {
    let failed = |what: &str, e: &dyn fmt::Display| InstallError::Failed(format!("{what}: {e}"));
    // ponytail: reads the whole update (~40 MB) so the verified bytes are the unpacked bytes
    let bytes = fs::read(&ready.path).map_err(|e| failed("reading the update", &e))?;
    update::check_file(&ready.manifest, bytes.as_slice()).map_err(|e| failed("refused the update", &e))?;
    let parent = bundle.parent().ok_or_else(|| failed("finding the app's folder", &bundle.display()))?;
    let staging = parent.join(format!(".glyphyard-update-{}", ready.manifest.version));
    // Also sweeps what an earlier install left behind (an old app, or a tar error file).
    for entry in fs::read_dir(parent).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().starts_with(".glyphyard-update-") {
            let path = entry.path();
            let _ = fs::remove_dir_all(&path).or_else(|_| fs::remove_file(&path));
        }
    }
    fs::create_dir(&staging).map_err(|e| failed("creating the staging folder", &e))?;
    let result = unpack(&bytes, &staging).and_then(|new| {
        swap(bundle, &new).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => InstallError::Blocked(format!("swapping in the new app: {e}")),
            _ => failed("swapping in the new app", &e),
        })
    });
    // After a swap this holds the old app.
    let _ = fs::remove_dir_all(&staging);
    result
}

fn unpack(bytes: &[u8], staging: &Path) -> Result<PathBuf, InstallError> {
    let failed = |e: String| InstallError::Failed(format!("unpacking the update: {e}"));
    let tar = if cfg!(target_os = "macos") { "/usr/bin/tar" } else { "tar" };
    // tar's stderr goes to a file beside the staging folder (not inside it), read after tar exits: no pipe to drain.
    let mut err_path = staging.as_os_str().to_owned();
    err_path.push(".tar-err");
    let err_path = PathBuf::from(err_path);
    let err_file = fs::File::create(&err_path).map(Stdio::from).unwrap_or_else(|_| Stdio::null());
    let mut child = Command::new(tar)
        .arg("-xzf")
        .arg("-")
        .arg("-C")
        .arg(staging)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(err_file)
        .spawn()
        .map_err(|e| failed(e.to_string()))?;
    let written = child.stdin.take().map(|mut stdin| stdin.write_all(bytes));
    let status = child.wait();
    let tar_err = fs::read_to_string(&err_path).unwrap_or_default();
    let _ = fs::remove_file(&err_path);
    let status = status.map_err(|e| failed(e.to_string()))?;
    if let Some(Err(e)) = written {
        return Err(failed(e.to_string()));
    }
    if !status.success() {
        let tar_err: String = tar_err.trim().chars().take(500).collect();
        return Err(failed(format!("tar exited with {status}: {tar_err}")));
    }
    new_app(staging).ok_or_else(|| failed("it does not hold exactly one app".into()))
}

/// The unpacked app: the staging folder's only entry, a `*.app` with `Contents/MacOS/typeshelf`.
pub fn new_app(staging: &Path) -> Option<PathBuf> {
    let mut entries = fs::read_dir(staging).ok()?;
    let app = entries.next()?.ok()?.path();
    let ok = entries.next().is_none()
        && app.file_name()?.to_str()?.ends_with(".app")
        && app.join("Contents").join("MacOS").join("typeshelf").is_file();
    ok.then_some(app)
}

/// Exchanges `a` and `b` in one atomic rename: there is never a moment without an app at either path.
#[cfg(target_os = "macos")]
fn swap(a: &Path, b: &Path) -> std::io::Result<()> {
    use std::ffi::{c_char, c_int, c_uint, CString};
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        fn renamex_np(from: *const c_char, to: *const c_char, flags: c_uint) -> c_int;
    }
    const RENAME_SWAP: c_uint = 0x2;
    let a = CString::new(a.as_os_str().as_bytes())?;
    let b = CString::new(b.as_os_str().as_bytes())?;
    // SAFETY: both are NUL-terminated strings that outlive the call.
    if unsafe { renamex_np(a.as_ptr(), b.as_ptr(), RENAME_SWAP) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Elsewhere (tests): three renames, restoring `a` if `b` cannot take its place.
// ponytail: not atomic, and a failing third rename leaves the old app at `a.swap`; only macOS installs for real.
#[cfg(not(target_os = "macos"))]
fn swap(a: &Path, b: &Path) -> std::io::Result<()> {
    let mut aside = a.as_os_str().to_owned();
    aside.push(".swap");
    let aside = PathBuf::from(aside);
    fs::rename(a, &aside)?;
    if let Err(e) = fs::rename(b, a) {
        let _ = fs::rename(&aside, a);
        return Err(e);
    }
    fs::rename(&aside, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use typeshelf_core::update::{Manifest, Platform, Version};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("typeshelf-mac-install-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fake_app(dir: &Path, name: &str, bin: &[u8]) -> PathBuf {
        let app = dir.join(name);
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/MacOS/typeshelf"), bin).unwrap();
        app
    }

    fn tarball(src: &Path, names: &[&str]) -> Vec<u8> {
        let out = std::process::Command::new("tar").arg("-czf").arg("-").arg("-C").arg(src).args(names).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        out.stdout
    }

    /// The update file as `check` leaves it: `<root>/update/Glyphyard-9.0.0.app.tar.gz` with its manifest.
    fn ready(root: &Path, file: &[u8], manifest_of: &[u8]) -> Ready {
        let dir = root.join("update");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Glyphyard-9.0.0.app.tar.gz");
        std::fs::write(&path, file).unwrap();
        Ready { path, manifest: Manifest::for_bytes(Version::parse("9.0.0").unwrap(), Platform::Macos, manifest_of) }
    }

    fn no_staging(parent: &Path) -> bool {
        std::fs::read_dir(parent).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(".glyphyard-update-"))
    }

    fn bin(app: &Path) -> Vec<u8> {
        std::fs::read(app.join("Contents/MacOS/typeshelf")).unwrap()
    }

    #[test]
    fn bundle_path_comes_from_the_exe() {
        assert_eq!(bundle_of(Path::new("/Applications/Glyphyard.app/Contents/MacOS/typeshelf")), Some(PathBuf::from("/Applications/Glyphyard.app")));
        assert_eq!(bundle_of(Path::new("/usr/local/bin/typeshelf")), None);
    }

    #[test]
    fn a_verified_tarball_replaces_the_bundle() {
        let root = temp("replace");
        let apps = root.join("Applications");
        let old = fake_app(&apps, "Glyphyard.app", b"old");
        fake_app(&root.join("src"), "Glyphyard.app", b"new");
        let bytes = tarball(&root.join("src"), &["Glyphyard.app"]);
        let ready = ready(&root, &bytes, &bytes);
        install(&ready, &old).unwrap();
        assert_eq!(bin(&old), b"new");
        assert!(no_staging(&apps));
        assert!(!ready.path.exists());
    }

    #[test]
    fn leftover_staging_folders_are_swept() {
        let root = temp("sweep");
        let apps = root.join("Applications");
        let old = fake_app(&apps, "Glyphyard.app", b"old");
        fake_app(&apps, ".glyphyard-update-0.0.1", b"ancient");
        fake_app(&root.join("src"), "Glyphyard.app", b"new");
        let bytes = tarball(&root.join("src"), &["Glyphyard.app"]);
        let ready = ready(&root, &bytes, &bytes);
        install(&ready, &old).unwrap();
        assert_eq!(bin(&old), b"new");
        assert!(no_staging(&apps));
    }

    #[test]
    fn a_tampered_tarball_changes_nothing() {
        let root = temp("tampered");
        let apps = root.join("Applications");
        let old = fake_app(&apps, "Glyphyard.app", b"old");
        fake_app(&root.join("src"), "Glyphyard.app", b"new");
        let bytes = tarball(&root.join("src"), &["Glyphyard.app"]);
        let ready = ready(&root, &bytes, b"what the manifest describes");
        assert!(matches!(install(&ready, &old), Err(InstallError::Failed(_))));
        assert_eq!(bin(&old), b"old");
        assert!(no_staging(&apps));
        assert!(!ready.path.exists());
    }

    #[test]
    fn a_tarball_without_exactly_one_app_changes_nothing() {
        let root = temp("notoneapp");
        let apps = root.join("Applications");
        let old = fake_app(&apps, "Glyphyard.app", b"old");
        let src = root.join("src");
        fake_app(&src, "Glyphyard.app", b"new");
        fake_app(&src, "Other.app", b"other");
        std::fs::create_dir_all(src.join("Empty.app/Contents")).unwrap();
        for names in [&["Glyphyard.app", "Other.app"][..], &["Empty.app"][..]] {
            let bytes = tarball(&src, names);
            let ready = ready(&root, &bytes, &bytes);
            assert!(matches!(install(&ready, &old), Err(InstallError::Failed(_))), "{names:?}");
            assert_eq!(bin(&old), b"old");
            assert!(no_staging(&apps));
        }
    }

    #[test]
    fn a_failed_swap_keeps_the_old_bundle() {
        let root = temp("swap");
        let existing = fake_app(&root, "Glyphyard.app", b"old");
        assert!(swap(&root.join("Missing.app"), &existing).is_err());
        assert_eq!(bin(&existing), b"old");
    }

    #[test]
    fn an_unwritable_folder_cannot_be_replaced() {
        let root = temp("readonly");
        let apps = root.join("Applications");
        let app = fake_app(&apps, "Glyphyard.app", b"old");
        assert!(can_replace(&app));
        let mut perms = std::fs::metadata(&apps).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&apps, perms.clone()).unwrap();
        let replaceable = can_replace(&app);
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&apps, perms).unwrap();
        assert!(!replaceable);
    }
}
