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

/// The baked-in public key; `None` (updates off, setting hidden) when the build has none or it is malformed.
pub fn key() -> Option<[u8; 32]> {
    update::public_key(UPDATE_KEY)
}

/// A downloaded setup program that passed every check.
#[derive(Debug, Clone)]
pub struct Ready {
    pub path: PathBuf,
    pub manifest: Manifest,
}

/// Managed state: the release waiting for quit.
#[derive(Default)]
pub struct Pending(Mutex<Option<Ready>>);

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
}

pub fn view(app: &AppHandle, settings: AppSettings) -> SettingsView {
    let update_ready = app.state::<Pending>().0.lock().unwrap().as_ref().map(|r| r.manifest.version.to_string());
    SettingsView { settings, can_update: key().is_some(), update_ready }
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
    // Leftovers from an earlier run (an installed release, an interrupted download) are never reused.
    let _ = std::fs::remove_dir_all(&dir);
    let mut wait = Duration::from_secs(60);
    loop {
        tokio::time::sleep(wait).await;
        wait = Duration::from_secs(24 * 60 * 60);
        if !ctx.run(|e| e.get_settings()).await.is_ok_and(|s| s.auto_update) {
            continue;
        }
        let pending = app.state::<Pending>();
        let have = pending.0.lock().unwrap().as_ref().map(|r| r.manifest.version);
        match check(&ctx.client(), Some(&key), &dir, CURRENT, have).await {
            Ok(Some(ready)) => {
                tracing::info!(version = %ready.manifest.version, "update downloaded and verified; it installs when Typeshelf quits");
                if let Some(old) = pending.0.lock().unwrap().replace(ready) {
                    let _ = std::fs::remove_file(old.path);
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("update check: {e}");
                wait = Duration::from_secs(60 * 60);
            }
        }
    }
}

/// One check. Does nothing without a key. `have` is the version already downloaded, if any.
pub async fn check(client: &SyncClient, key: Option<&[u8; 32]>, dir: &Path, current: &str, have: Option<update::Version>) -> Result<Option<Ready>, String> {
    let Some(key) = key else { return Ok(None) };
    let Some((text, signature)) = client.update_release().await.map_err(|e| e.to_string())? else { return Ok(None) };
    let manifest = update::verify_manifest(key, text.as_bytes(), &signature).map_err(|e| format!("refused the release: {e}"))?;
    if !manifest.is_newer_than(current) || have == Some(manifest.version) {
        return Ok(None);
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let part = dir.join("Typeshelf-Setup.part");
    client.download_update(&part, manifest.size).await.map_err(|e| format!("downloading the update: {e}"))?;
    let dir = dir.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || finish(&part, manifest, &dir))
        .await
        .map_err(|e| e.to_string())?
        .map(Some)
        .map_err(|e| format!("refused the download: {e}"))
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

fn io(e: std::io::Error) -> UpdateError {
    UpdateError::Io(e.to_string())
}

/// At quit, from `shutdown`: starts the waiting release when updates are still on.
pub fn on_quit(app: &AppHandle) {
    let Some(ready) = app.state::<Pending>().0.lock().unwrap().take() else { return };
    let settings = app.state::<Ctx>().engine.get_settings();
    if key().is_none() || !settings.as_ref().is_ok_and(|s| s.auto_update) {
        let _ = std::fs::remove_file(&ready.path);
        return;
    }
    let Some(install_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) else { return };
    // Silent Setup turns "start at login" on unless told otherwise, so pass the current choice along.
    let mut args: Vec<OsString> = vec!["--silent".into(), "--dir".into(), install_dir.into()];
    if !settings.is_ok_and(|s| s.start_at_login) {
        args.push("--no-start-at-login".into());
    }
    match launch(&ready, &args) {
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

    fn tempfile_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("typeshelf-update-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
