//! Updates from the team server. Off unless the build carries an update public key
//! (`TYPESHELF_UPDATE_KEY`, baked in by scripts/build-windows.ps1) and Settings › "Update automatically" is on.
//!
//! Checks a minute after start, then daily: fetch the manifest and signature, verify the signature with the
//! baked-in key, ignore anything not newer than this app, download the setup program into `<data>/update/`,
//! check it against the manifest's size and hash, and only then mark it ready. At quit the file is checked
//! again (it sat on disk) while a handle denies writes to it, and started detached with `--silent`.
//! Any failure deletes the file and is logged; nothing unverified is ever started.

use std::ffi::OsString;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use typeshelf_core::update::{self, Manifest, UpdateError};
use typeshelf_engine::AppSettings;
use typeshelf_sync::SyncClient;

use crate::Ctx;

const UPDATE_KEY: Option<&str> = option_env!("TYPESHELF_UPDATE_KEY");
const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// The baked-in public key; `None` (updates off, setting hidden) when the build has none or it is malformed, and
/// on every system but Windows (the update is a Windows Setup program).
pub fn key() -> Option<[u8; 32]> {
    if !cfg!(windows) {
        return None;
    }
    update::public_key(UPDATE_KEY)
}

/// A downloaded setup program that passed every check.
#[derive(Debug, Clone)]
pub struct Ready {
    pub path: PathBuf,
    pub manifest: Manifest,
}

/// Managed state: the release waiting for quit, and whether the last check refused a release.
#[derive(Default)]
pub struct Pending {
    ready: Mutex<Option<Ready>>,
    problem: Mutex<Option<String>>,
}

/// Shown in Settings when the server offered a release that failed verification.
const PROBLEM: &str = "The update from your server could not be verified and was not installed. Tell your admin: the update may be signed with a different key than this app was built with.";

/// Why a check stopped.
#[derive(Debug)]
pub enum CheckError {
    /// The signature, manifest or downloaded file failed verification: shown in Settings.
    Refused(String),
    /// Offline, server or disk trouble: logged and retried.
    Failed(String),
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            CheckError::Refused(e) | CheckError::Failed(e) => f.write_str(e),
        }
    }
}

/// The settings response plus what the UI needs to know about updates.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    #[serde(flatten)]
    settings: AppSettings,
    /// False when the build has no update key: the switch is hidden.
    can_update: bool,
    /// The version that will be installed at quit.
    update_ready: Option<String>,
    /// Set when the last check refused the server's release (bad signature or file).
    update_problem: Option<String>,
}

pub fn view(app: &AppHandle, settings: AppSettings) -> SettingsView {
    let pending = app.state::<Pending>();
    let update_ready = pending.ready.lock().unwrap().as_ref().map(|r| r.manifest.version.to_string());
    let update_problem = pending.problem.lock().unwrap().clone();
    SettingsView { settings, can_update: key().is_some(), update_ready, update_problem }
}

fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("update")
}

pub async fn run(app: AppHandle) {
    let Some(key) = key() else {
        tracing::info!("this build has no update key, so it never checks for updates");
        return;
    };
    let ctx = app.state::<Ctx>();
    let dir = dir(&ctx.data_dir);
    // An interrupted download is never resumed. A finished installer is kept: `check` reuses it when it is still
    // the server's release and matches its manifest, and deletes it otherwise.
    let _ = std::fs::remove_file(dir.join("Typeshelf-Setup.part"));
    let mut wait = Duration::from_secs(60);
    loop {
        tokio::time::sleep(wait).await;
        wait = Duration::from_secs(24 * 60 * 60);
        let gate = ctx.run(|e| Ok(should_check(e.get_settings()?.auto_update, e.get_app_state()?.server_configured, e.token()?.is_some()))).await;
        if !gate.unwrap_or(false) {
            continue;
        }
        let pending = app.state::<Pending>();
        let have = pending.ready.lock().unwrap().as_ref().map(|r| r.manifest.version);
        match check(&ctx.client(), Some(&key), &dir, CURRENT, have).await {
            Ok(found) => {
                *pending.problem.lock().unwrap() = None;
                if let Some(ready) = found {
                    tracing::info!(version = %ready.manifest.version, "update downloaded and verified; it installs when Glyphyard quits");
                    if let Some(old) = pending.ready.lock().unwrap().replace(ready) {
                        let _ = std::fs::remove_file(old.path);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("update check: {e}");
                if matches!(e, CheckError::Refused(_)) {
                    *pending.problem.lock().unwrap() = Some(PROBLEM.into());
                }
                wait = Duration::from_secs(60 * 60);
            }
        }
    }
}

/// One check. Does nothing without a key. `have` is the version already downloaded, if any.
pub async fn check(client: &SyncClient, key: Option<&[u8; 32]>, dir: &Path, current: &str, have: Option<update::Version>) -> Result<Option<Ready>, CheckError> {
    let failed = |e: String| CheckError::Failed(e);
    let Some(key) = key else { return Ok(None) };
    let Some((text, signature)) = client.update_release().await.map_err(|e| failed(e.to_string()))? else { return Ok(None) };
    let manifest = update::verify_manifest(key, text.as_bytes(), &signature).map_err(|e| CheckError::Refused(format!("refused the release: {e}")))?;
    if !wanted(&manifest, current, have) {
        if have.is_none() {
            // Nothing waiting and nothing newer: what is left (the release already installed) is stale.
            let _ = std::fs::remove_dir_all(dir);
        }
        return Ok(None);
    }
    let (d, m) = (dir.to_path_buf(), manifest.clone());
    if let Some(ready) = tauri::async_runtime::spawn_blocking(move || reuse(&d, &m)).await.map_err(|e| failed(e.to_string()))? {
        return Ok(Some(ready));
    }
    std::fs::create_dir_all(dir).map_err(|e| failed(e.to_string()))?;
    let part = dir.join("Typeshelf-Setup.part");
    client.download_update(&part, manifest.size).await.map_err(|e| failed(format!("downloading the update: {e}")))?;
    let dir = dir.to_path_buf();
    match tauri::async_runtime::spawn_blocking(move || finish(&part, manifest, &dir)).await.map_err(|e| failed(e.to_string()))? {
        Ok(ready) => Ok(Some(ready)),
        Err(UpdateError::Io(e)) => Err(failed(format!("checking the download: {e}"))),
        Err(e) => Err(CheckError::Refused(format!("refused the download: {e}"))),
    }
}

/// Checks a finished download against the verified manifest; deletes it on any mismatch.
pub fn finish(part: &Path, manifest: Manifest, dir: &Path) -> Result<Ready, UpdateError> {
    let checked = File::open(part).map_err(io).and_then(|f| update::check_file(&manifest, f));
    if let Err(e) = checked {
        let _ = std::fs::remove_file(part);
        return Err(e);
    }
    let path = dir.join(format!("Typeshelf-Setup-{}.exe", manifest.version));
    std::fs::rename(part, &path).map_err(io)?;
    Ok(Ready { path, manifest })
}

/// The installer of this release left by an earlier run, when it still matches the (freshly verified) manifest;
/// `launch` checks it again at quit. Every other file in the folder (an older or tampered installer) is deleted.
pub fn reuse(dir: &Path, manifest: &Manifest) -> Option<Ready> {
    let path = dir.join(format!("Typeshelf-Setup-{}.exe", manifest.version));
    let ok = File::open(&path).map_err(io).and_then(|f| update::check_file(manifest, f)).is_ok();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if !ok || entry.path() != path {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    ok.then(|| Ready { path, manifest: manifest.clone() })
}

/// Whether a scheduled check talks to the server. Standalone (no server) or not registered yet (no token):
/// no request at all, the same condition the sync loop uses before it connects.
pub fn should_check(auto_update: bool, server_configured: bool, registered: bool) -> bool {
    auto_update && server_configured && registered
}

/// A verified release is downloaded only when it is newer than this app and not the one already waiting.
pub fn wanted(manifest: &Manifest, current: &str, have: Option<update::Version>) -> bool {
    manifest.is_newer_than(current) && have != Some(manifest.version)
}

/// Glyphyard Setup and the uninstaller hold this mutex for their whole run (apps/setup/src/main.rs).
pub const SETUP_MUTEX: &str = "Local\\TypeshelfSetupRunning";

/// Whether to start the waiting release at quit. Not when a Setup or uninstall is the one closing the app.
pub fn should_launch(has_key: bool, auto_update: bool, setup_running: bool) -> bool {
    has_key && auto_update && !setup_running
}

/// True while a Glyphyard Setup or uninstall runs; also true when that cannot be told (refuse, don't guess).
// ponytail: an older Setup without the mutex is not seen; it closes the app and the pending update starts too.
#[cfg(windows)]
pub fn setup_running() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenMutexW(access: u32, inherit: i32, name: *const u16) -> isize;
        fn CloseHandle(handle: isize) -> i32;
        fn GetLastError() -> u32;
    }
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const ERROR_FILE_NOT_FOUND: u32 = 2;
    let name: Vec<u16> = SETUP_MUTEX.encode_utf16().chain([0]).collect();
    // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call; the handle is closed once.
    unsafe {
        let handle = OpenMutexW(SYNCHRONIZE, 0, name.as_ptr());
        if handle != 0 {
            CloseHandle(handle);
            return true;
        }
        GetLastError() != ERROR_FILE_NOT_FOUND
    }
}

/// Setup is Windows-only.
#[cfg(not(windows))]
pub fn setup_running() -> bool {
    false
}

fn io(e: std::io::Error) -> UpdateError {
    UpdateError::Io(e.to_string())
}

/// At quit, from `shutdown`: starts the waiting release when updates are still on and no Glyphyard Setup or
/// uninstall is the one closing the app. Silent Setup updates the registered install folder in place.
// ponytail: only a real quit installs; with "Start in the tray" on, closing the window only hides it, so a tray-mode
// user installs the update at their next explicit Quit (the file is kept and reused across starts until then).
pub fn on_quit(app: &AppHandle) {
    let Some(ready) = app.state::<Pending>().ready.lock().unwrap().take() else { return };
    let auto_update = app.state::<Ctx>().engine.get_settings().is_ok_and(|s| s.auto_update);
    if !should_launch(key().is_some(), auto_update, setup_running()) {
        tracing::info!(version = %ready.manifest.version, "not installing the waiting update (updates off, or Setup is running)");
        let _ = std::fs::remove_file(&ready.path);
        return;
    }
    match launch(&ready, &["--silent".into()]) {
        Ok(()) => tracing::info!(version = %ready.manifest.version, "started the update"),
        Err(e) => tracing::warn!("not installing the update: {e}"),
    }
}

/// Re-checks the file while holding a handle that lets nobody else write or delete it, then starts it
/// detached so it outlives the app. A file that no longer matches is deleted, never started.
pub fn launch(ready: &Ready, args: &[OsString]) -> Result<(), UpdateError> {
    let mut open = std::fs::OpenOptions::new();
    open.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        open.share_mode(FILE_SHARE_READ);
    }
    let file = open.open(&ready.path).map_err(io)?;
    if let Err(e) = update::check_file(&ready.manifest, &file) {
        drop(file);
        let _ = std::fs::remove_file(&ready.path);
        return Err(e);
    }
    let mut cmd = std::process::Command::new(&ready.path);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x8;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().map_err(io)?;
    drop(file);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use typeshelf_core::update::Version;
    use typeshelf_sync::Config;

    fn manifest_for(bytes: &[u8]) -> Manifest {
        Manifest::for_bytes(Version::parse("9.0.0").unwrap(), bytes)
    }

    #[test]
    fn standalone_or_unregistered_never_checks() {
        assert!(!should_check(true, false, false), "no server configured");
        assert!(!should_check(true, false, true), "no server configured, an old token");
        assert!(!should_check(true, true, false), "not registered yet");
        assert!(!should_check(false, true, true), "updates off");
        assert!(should_check(true, true, true));
    }

    #[test]
    fn a_build_without_a_key_does_nothing() {
        let dir = tempfile_dir("nokey");
        // A server that would refuse everything; with no key it must not even be asked.
        let client = SyncClient::new(Config { server_url: "http://127.0.0.1:9".into(), install_key: String::new(), token: None, outbox_path: dir.join("outbox") });
        let got = tauri::async_runtime::block_on(check(&client, None, &dir.join("update"), "0.1.0", None));
        assert!(matches!(got, Ok(None)));
        assert!(!dir.join("update").exists());
        if UPDATE_KEY.is_none() {
            assert!(key().is_none());
        }
    }

    #[test]
    fn a_download_that_does_not_match_is_deleted() {
        let dir = tempfile_dir("finish");
        let part = dir.join("Typeshelf-Setup.part");
        std::fs::write(&part, b"tampered").unwrap();
        assert_eq!(finish(&part, manifest_for(b"original"), &dir).unwrap_err(), UpdateError::HashMismatch);
        assert!(!part.exists());

        std::fs::write(&part, b"original").unwrap();
        let ready = finish(&part, manifest_for(b"original"), &dir).unwrap();
        assert_eq!(ready.path, dir.join("Typeshelf-Setup-9.0.0.exe"));
        assert!(ready.path.exists() && !part.exists());
    }

    #[test]
    fn a_verified_installer_from_an_earlier_run_is_reused() {
        let dir = tempfile_dir("reuse");
        let path = dir.join("Typeshelf-Setup-9.0.0.exe");
        let older = dir.join("Typeshelf-Setup-8.0.0.exe");
        std::fs::write(&path, b"original").unwrap();
        std::fs::write(&older, b"older").unwrap();
        let ready = reuse(&dir, &manifest_for(b"original")).expect("the matching file is reused");
        assert_eq!(ready.path, path);
        assert!(path.exists(), "kept for the launch at quit");
        assert!(!older.exists(), "any other leftover installer is deleted");

        // Present but not the release the manifest describes: deleted, so it is downloaded again.
        std::fs::write(&path, b"tampered").unwrap();
        assert!(reuse(&dir, &manifest_for(b"original")).is_none());
        assert!(!path.exists());

        // Nothing there: downloaded.
        assert!(reuse(&dir, &manifest_for(b"original")).is_none());
    }

    #[test]
    fn a_file_changed_after_verification_is_never_started() {
        let dir = tempfile_dir("launch");
        let path = dir.join("Typeshelf-Setup-9.0.0.exe");
        std::fs::write(&path, b"original").unwrap();
        let ready = Ready { path: path.clone(), manifest: manifest_for(b"original") };
        std::fs::write(&path, b"swapped!").unwrap();
        assert_eq!(launch(&ready, &[]).unwrap_err(), UpdateError::HashMismatch);
        assert!(!path.exists());
    }

    /// The verified file is started while the handle that blocks writes is still open.
    #[cfg(windows)]
    #[test]
    fn a_verified_file_is_started() {
        let dir = tempfile_dir("start");
        let bytes = std::fs::read(r"C:\Windows\System32\whoami.exe").unwrap();
        let path = dir.join("Typeshelf-Setup-9.0.0.exe");
        std::fs::write(&path, &bytes).unwrap();
        launch(&Ready { path, manifest: manifest_for(&bytes) }, &["/?".into()]).unwrap();
    }

    #[test]
    fn only_a_newer_release_than_this_app_is_wanted() {
        let (private, public) = update::generate_key().unwrap();
        let key = update::public_key(Some(&public)).unwrap();
        let verified = |v: &str| {
            let text = Manifest::for_bytes(Version::parse(v).unwrap(), b"file").to_text();
            let (sig, _) = update::sign(&private, text.as_bytes()).unwrap();
            update::verify_manifest(&key, text.as_bytes(), &sig).unwrap()
        };
        assert!(!wanted(&verified("0.1.1"), "0.1.2", None), "older");
        assert!(!wanted(&verified("0.1.2"), "0.1.2", None), "same");
        assert!(wanted(&verified("0.1.3"), "0.1.2", None), "newer");
        assert!(wanted(&verified("0.10.0"), "0.9.9", None), "numeric");
        let v = Version::parse("0.1.3").unwrap();
        assert!(!wanted(&verified("0.1.3"), "0.1.2", Some(v)), "already downloaded");
    }

    #[test]
    fn the_pending_update_starts_only_when_no_setup_is_closing_the_app() {
        assert!(should_launch(true, true, false));
        assert!(!should_launch(true, true, true), "a Setup or uninstall is running");
        assert!(!should_launch(true, false, false), "updates turned off");
        assert!(!should_launch(false, true, false), "no key");
    }

    /// The mutex Setup creates is seen; without it, nothing is running.
    #[cfg(windows)]
    #[test]
    fn a_running_setup_is_detected_by_its_mutex() {
        #[link(name = "kernel32")]
        extern "system" {
            fn CreateMutexW(attributes: *const std::ffi::c_void, owner: i32, name: *const u16) -> isize;
            fn CloseHandle(handle: isize) -> i32;
        }
        assert!(!setup_running());
        let name: Vec<u16> = SETUP_MUTEX.encode_utf16().chain([0]).collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        assert_ne!(handle, 0);
        assert!(setup_running());
        unsafe { CloseHandle(handle) };
        assert!(!setup_running());
    }

    fn tempfile_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("typeshelf-update-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
