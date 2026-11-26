//! Finds Adobe apps and installs/removes the Glyphyard plug-ins built by `bun plugins/build.ts`.
//! The commands are the ones documented in `plugins/README.md`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::App;

/// CEP runtimes used by the 2022 apps (CSXS 11) onwards.
const CSXS_VERSIONS: [u32; 2] = [11, 12];

#[derive(Clone, Debug, Serialize)]
pub struct AdobeApp {
    pub app: App,
    pub label: &'static str,
    /// Installed versions the plug-in supports, e.g. `["2025", "2026"]`.
    pub versions: Vec<String>,
    /// The Glyphyard plug-in is installed.
    pub installed: bool,
}

impl App {
    pub const ALL: [App; 3] = [App::Photoshop, App::Illustrator, App::Indesign];

    pub fn label(self) -> &'static str {
        match self {
            App::Photoshop => "Photoshop",
            App::Illustrator => "Illustrator",
            App::Indesign => "InDesign",
        }
    }

    /// Oldest release year the plug-in runs in (UXP manifest v5: PS 23.3; UXP InDesign: 18.5; CEP 11: AI 26).
    fn min_year(self) -> u32 {
        match self {
            App::Photoshop | App::Illustrator => 2022,
            App::Indesign => 2023,
        }
    }

    /// The `.ccx` in `dist/` and the names the Unified Plugin Installer Agent lists it under (manifest `name`):
    /// the current one, then the one from before the Typeshelf → Glyphyard rename (older installs).
    fn uxp(self) -> Option<(&'static str, [&'static str; 2])> {
        match self {
            App::Photoshop => Some(("typeshelf-photoshop.ccx", ["Glyphyard for Photoshop", "Typeshelf for Photoshop"])),
            App::Indesign => Some(("typeshelf-indesign.ccx", ["Glyphyard for InDesign", "Typeshelf for InDesign"])),
            App::Illustrator => None,
        }
    }

    /// Folder name of the app's CEP extension in `dist/` and in the CEP extensions folder.
    pub fn cep_id(self) -> &'static str {
        match self {
            App::Photoshop => "com.typeshelf.photoshop",
            App::Illustrator => "com.typeshelf.illustrator",
            App::Indesign => "com.typeshelf.indesign",
        }
    }
}

fn cep_installed(app: App) -> bool {
    cep_dir().is_some_and(|d| d.join(app.cep_id()).join("CSXS").join("manifest.xml").is_file())
}

/// Photoshop and InDesign get the UXP plug-in through Creative Cloud's installer when it is there; Adobe
/// apps installed without the Creative Cloud desktop app don't have it, so they get the CEP extension.
pub fn detect_apps() -> Vec<AdobeApp> {
    let uxp_list = upia(&["list", "all"]).unwrap_or_default();
    App::ALL
        .into_iter()
        .map(|app| AdobeApp {
            app,
            label: app.label(),
            versions: versions(app),
            installed: cep_installed(app) || app.uxp().is_some_and(|(_, names)| names.iter().any(|n| uxp_list.contains(n))),
        })
        .collect()
}

pub fn install(app: App, dist_dir: &Path) -> io::Result<()> {
    match app.uxp() {
        Some((ccx, names)) if upia_path().is_some_and(|p| p.is_file()) => {
            let ccx = dist_dir.join(ccx);
            if !ccx.is_file() {
                return Err(io::Error::new(io::ErrorKind::NotFound, format!("{} not found", ccx.display())));
            }
            let out = upia(&["install", &ccx.to_string_lossy()])?;
            // The agent's exit code is not a reliable success signal, so check the result.
            if !upia(&["list", "all"])?.contains(names[0]) {
                return Err(io::Error::other(format!("Adobe plug-in installer: {}", out.trim())));
            }
            Ok(())
        }
        _ => {
            let dest = cep_dir().ok_or_else(unsupported)?.join(app.cep_id());
            if dest.exists() {
                fs::remove_dir_all(&dest)?;
            }
            copy_dir(&dist_dir.join(app.cep_id()), &dest)?;
            enable_unsigned_cep()
        }
    }
}

/// Removes whichever form is installed (or both).
pub fn remove(app: App) -> io::Result<()> {
    let dir = cep_dir().ok_or_else(unsupported)?.join(app.cep_id());
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {} // PlayerDebugMode stays on: other unsigned extensions may rely on it.
    }
    if let Some((_, names)) = app.uxp() {
        let list = upia(&["list", "all"]).unwrap_or_default();
        for name in names.into_iter().filter(|n| list.contains(n)) {
            upia(&["remove", name])?;
        }
    }
    Ok(())
}

/// InDesign is open. Unlike Photoshop and Illustrator it only sees fonts turned on or off while it runs
/// after a restart (checked against InDesign 2024 on Windows, also with fonts installed by Windows itself).
pub fn indesign_running() -> bool {
    running(App::Indesign).unwrap_or(false)
}

/// Whether `app` has a process (`Photoshop.exe` etc. on Windows, a command line with "Adobe Photoshop" etc. on
/// macOS). An error means the process list could not be read. Blocks while the command runs.
pub fn running(app: App) -> io::Result<bool> {
    let exe = format!("{}.exe", app.label());
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("tasklist");
        c.args(["/FI", &format!("IMAGENAME eq {exe}"), "/FO", "CSV", "/NH"]);
        c
    } else {
        let mut c = Command::new("pgrep");
        c.args(["-f", &format!("Adobe {}", app.label())]);
        c
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output()?;
    match out.status.code() {
        // tasklist answers a non-match with an "INFO:" line; pgrep prints PIDs and exits 1 on no match.
        Some(0) if cfg!(windows) => Ok(String::from_utf8_lossy(&out.stdout).to_lowercase().contains(&exe.to_lowercase())),
        Some(0) => Ok(true),
        Some(1) if !cfg!(windows) => Ok(false),
        _ => Err(io::Error::other(format!("process list failed: {}", out.status))),
    }
}

/// Release years found in folders like `Adobe Photoshop 2026`.
fn versions(app: App) -> Vec<String> {
    let prefix = format!("Adobe {} ", app.label());
    let Some(entries) = apps_dir().and_then(|d| fs::read_dir(d).ok()) else {
        return Vec::new();
    };
    let mut years: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_prefix(&prefix)?.parse::<u32>().ok())
        .filter(|&y| y >= app.min_year())
        .map(|y| y.to_string())
        .collect();
    years.sort();
    years.dedup();
    years
}

fn apps_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        Some(env_dir("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".into()).join("Adobe"))
    } else if cfg!(target_os = "macos") {
        Some("/Applications".into())
    } else {
        None
    }
}

fn cep_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        Some(env_dir("APPDATA")?.join(r"Adobe\CEP\extensions"))
    } else if cfg!(target_os = "macos") {
        Some(env_dir("HOME")?.join("Library/Application Support/Adobe/CEP/extensions"))
    } else {
        None
    }
}

fn upia_path() -> Option<PathBuf> {
    const AGENT: &str = "Adobe/Adobe Desktop Common/RemoteComponents/UPI/UnifiedPluginInstallerAgent";
    if cfg!(windows) {
        let common = env_dir("CommonProgramFiles").unwrap_or_else(|| r"C:\Program Files\Common Files".into());
        Some(common.join(AGENT).join("UnifiedPluginInstallerAgent.exe"))
    } else if cfg!(target_os = "macos") {
        Some(
            Path::new("/Library/Application Support")
                .join(AGENT)
                .join("UnifiedPluginInstallerAgent.app/Contents/MacOS/UnifiedPluginInstallerAgent"),
        )
    } else {
        None
    }
}

/// Runs the Unified Plugin Installer Agent (ships with Creative Cloud). Windows spells options `/install`,
/// macOS `--install`. Returns stdout + stderr.
fn upia(args: &[&str]) -> io::Result<String> {
    let path = upia_path().filter(|p| p.is_file()).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Creative Cloud's plug-in installer was not found")
    })?;
    let mut cmd = Command::new(path);
    let flag = if cfg!(windows) { "/" } else { "--" };
    cmd.arg(format!("{flag}{}", args[0])).args(&args[1..]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output()?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(text)
    } else {
        Err(io::Error::other(format!("Adobe plug-in installer: {}", text.trim())))
    }
}

/// Unsigned CEP extensions only load with `PlayerDebugMode` = 1 for the app's CSXS version.
#[cfg(windows)]
fn enable_unsigned_cep() -> io::Result<()> {
    for v in CSXS_VERSIONS {
        windows_registry::CURRENT_USER
            .create(format!(r"Software\Adobe\CSXS.{v}"))
            .and_then(|key| key.set_string("PlayerDebugMode", "1"))
            .map_err(io::Error::other)?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn enable_unsigned_cep() -> io::Result<()> {
    for v in CSXS_VERSIONS {
        let domain = format!("com.adobe.CSXS.{v}");
        let status = Command::new("defaults").args(["write", &domain, "PlayerDebugMode", "1"]).status()?;
        if !status.success() {
            return Err(io::Error::other(format!("defaults write {domain} PlayerDebugMode 1 failed")));
        }
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn unsupported() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "Adobe plug-ins are only supported on Windows and macOS")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The command runs and its answer parses (no Adobe app here, so it can't check a positive answer).
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn process_check_answers() {
        assert!(running(App::Photoshop).is_ok());
    }

    #[test]
    fn copy_dir_copies_nested_files() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("CSXS")).unwrap();
        fs::write(src.join("CSXS/manifest.xml"), "m").unwrap();
        fs::write(src.join("index.html"), "i").unwrap();
        copy_dir(&src, &tmp.path().join("dst")).unwrap();
        assert_eq!(fs::read_to_string(tmp.path().join("dst/CSXS/manifest.xml")).unwrap(), "m");
        assert_eq!(fs::read_to_string(tmp.path().join("dst/index.html")).unwrap(), "i");
    }
}
