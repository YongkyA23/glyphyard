//! Glyphyard Setup: per-user installer, updater and uninstaller (see docs/setup.md).

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod install;
mod payload;
mod process;
mod registry;
mod shortcuts;
mod uninstall;

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{exit, Child, Command};
use std::sync::Mutex;

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tauri_plugin_dialog::DialogExt;

pub const VERSION: &str = env!("TYPESHELF_VERSION");
const SERVER_URL: Option<&str> = option_env!("TYPESHELF_SERVER_URL");
const WEBVIEW2_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

#[derive(Debug, Serialize)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}

impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Error { code, message: message.into() }
    }

    pub fn app_running() -> Self {
        Error::new("app_running", "Glyphyard is running. Close it to continue.")
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::new("io", e.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// Appends to %TEMP%\typeshelf-setup.log (every mode; it's the only output of --silent).
pub fn log(msg: impl AsRef<str>) {
    let path = std::env::temp_dir().join("typeshelf-setup.log");
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} [{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), std::process::id(), msg.as_ref());
    }
}

#[derive(Default)]
struct Args {
    raw: Vec<String>,
    uninstall: bool,
    silent: bool,
    /// After a successful silent install, start the installed app (the desktop app's "Restart now").
    launch: bool,
    wipe_data: bool,
    desktop: bool,
    no_start_menu: bool,
    no_start_at_login: bool,
    dir: Option<PathBuf>,
    /// Set when uninstall.exe relaunched itself from %TEMP%: the folder being uninstalled.
    from: Option<PathBuf>,
}

fn parse_args() -> Args {
    parse(std::env::args().skip(1).collect())
}

fn parse(raw: Vec<String>) -> Args {
    let mut a = Args { raw: raw.clone(), ..Args::default() };
    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--uninstall" => a.uninstall = true,
            "--silent" => a.silent = true,
            "--launch" => a.launch = true,
            "--wipe-data" => a.wipe_data = true,
            "--desktop" => a.desktop = true,
            "--no-start-menu" => a.no_start_menu = true,
            "--no-start-at-login" => a.no_start_at_login = true,
            "--dir" => a.dir = it.next().map(PathBuf::from),
            "--from" => a.from = it.next().map(PathBuf::from),
            _ => {}
        }
    }
    a
}

struct Setup {
    mode: &'static str,
    /// Default install folder, or the folder being uninstalled.
    dir: PathBuf,
    installed_version: Option<String>,
    fonts_active: Option<usize>,
    /// Folder of the last successful install, for `launch_app`.
    installed_to: Mutex<Option<PathBuf>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Info {
    mode: &'static str,
    version: &'static str,
    installed_version: Option<String>,
    default_dir: PathBuf,
    server_url: &'static str,
    size_mb: f64,
    fonts_active: Option<usize>,
}

#[tauri::command]
fn info(state: State<Setup>) -> Info {
    Info {
        mode: state.mode,
        version: VERSION,
        installed_version: state.installed_version.clone(),
        default_dir: state.dir.clone(),
        server_url: SERVER_URL.unwrap_or_default(),
        size_mb: (install::total_size() as f64 / 1_048_576.0 * 10.0).round() / 10.0,
        fonts_active: state.fonts_active,
    }
}

/// Returns the picked folder with "Typeshelf" appended (uninstall deletes the whole install folder). The
/// install folder keeps its pre-rename name.
#[tauri::command]
async fn pick_dir(app: AppHandle, current: String) -> Option<PathBuf> {
    let mut dialog = app.dialog().file().set_title("Choose where to install Glyphyard");
    if let Some(start) = Path::new(&current).ancestors().find(|p| p.is_dir()) {
        dialog = dialog.set_directory(start);
    }
    let picked = dialog.blocking_pick_folder()?.into_path().ok()?;
    let is_ours = picked.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Typeshelf"));
    Some(if is_ours { picked } else { picked.join("Typeshelf") })
}

#[tauri::command]
async fn install(app: AppHandle, dir: PathBuf, start_menu: bool, desktop: bool, start_at_login: bool) -> Result<(), Error> {
    let opts = install::Options { dir, start_menu, desktop, start_at_login };
    let h = app.clone();
    let dir = opts.dir.clone();
    blocking(move || install::install(&opts, &|pct, step| emit_progress(&h, pct, step))).await?;
    *app.state::<Setup>().installed_to.lock().unwrap() = Some(dir);
    Ok(())
}

#[tauri::command]
async fn uninstall(app: AppHandle, wipe_data: bool) -> Result<uninstall::Report, Error> {
    let dir = app.state::<Setup>().dir.clone();
    blocking(move || uninstall::uninstall(&dir, wipe_data, &|pct, step| emit_progress(&app, pct, step))).await
}

#[tauri::command]
async fn close_app() -> Result<(), Error> {
    blocking(|| {
        process::close_app();
        Ok(())
    })
    .await
}

#[tauri::command]
fn launch_app(app: AppHandle, state: State<Setup>) -> Result<(), Error> {
    let dir = state.installed_to.lock().unwrap().clone().unwrap_or_else(|| state.dir.clone());
    process::launch_app(&dir)?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

fn emit_progress(app: &AppHandle, pct: u32, step: &str) {
    let _ = app.emit("progress", json!({ "pct": pct.min(100), "step": step }));
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, Error> + Send + 'static) -> Result<T, Error> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| Error::new("io", e.to_string()))?
}

fn main() {
    // For the whole run, before anything can close the app (the uninstall relaunch from %TEMP% takes it too).
    process::hold_setup_mutex();
    let args = parse_args();
    let uninstall_dir = args.uninstall.then(|| find_install(&args));
    if let (Some(dir), None) = (&uninstall_dir, &args.from) {
        if running_inside(dir) {
            match relaunch_from_temp(&args, dir) {
                // --silent callers read the exit code, so wait for the copy that does the work.
                Ok(mut child) if args.silent => exit(child.wait().ok().and_then(|s| s.code()).unwrap_or(1)),
                Ok(_) => exit(0),
                Err(e) => log(format!("couldn't relaunch from %TEMP%, uninstall.exe will stay behind: {e}")),
            }
        }
    }
    if args.silent {
        let code = silent(&args, uninstall_dir);
        cleanup(&args, false);
        exit(code);
    }
    gui(args, uninstall_dir);
}

fn silent(args: &Args, uninstall_dir: Option<PathBuf>) -> i32 {
    let progress = |pct: u32, step: &str| log(format!("{step} {pct}%"));
    // Same as the GUI's "Close Glyphyard" button (asks nicely, forces after 5 s).
    if process::app_running() {
        log("closing Glyphyard");
        process::close_app();
    }
    let is_install = uninstall_dir.is_none();
    let install_dir = args.dir.clone().unwrap_or_else(install::default_dir);
    let result = match uninstall_dir {
        Some(dir) => uninstall::uninstall(&dir, args.wipe_data, &progress).map(|_| None),
        None => {
            let opts = install::Options {
                dir: install_dir.clone(),
                start_menu: !args.no_start_menu,
                desktop: args.desktop,
                start_at_login: !args.no_start_at_login,
            };
            install::install(&opts, &progress).map(|()| Some(opts.dir))
        }
    };
    match result {
        Ok(installed) => {
            // Ignored after an uninstall. A failed launch doesn't fail the install.
            if let (true, Some(dir)) = (args.launch, installed) {
                match process::launch_app(&dir) {
                    Ok(()) => log("launched Glyphyard"),
                    Err(e) => log(format!("couldn't launch Glyphyard: {e}")),
                }
            }
            0
        }
        Err(e) => {
            log(format!("failed: {e}"));
            // The previous version is left intact on a failure, so a restart still gets an app back.
            let exe_exists = install_dir.join(process::APP_EXE).is_file();
            if launch_after(false, args.launch, is_install, exe_exists, process::app_running()) {
                match process::launch_app(&install_dir) {
                    Ok(()) => log("launched the previous Glyphyard"),
                    Err(e) => log(format!("couldn't launch Glyphyard: {e}")),
                }
            }
            1
        }
    }
}

/// Whether `--launch` should start the app: after a good install, or after a failed one that left a previous
/// version that is not already running.
fn launch_after(ok: bool, launch: bool, is_install: bool, exe_exists: bool, running: bool) -> bool {
    launch && is_install && (ok || (exe_exists && !running))
}

fn gui(args: Args, uninstall_dir: Option<PathBuf>) {
    // Setup's own UI needs WebView2, so check before creating any window.
    if install::webview2_version().is_none() {
        log("WebView2 runtime missing");
        webview2_missing();
        exit(1);
    }
    let installed = registry::installed();
    let state = Setup {
        mode: if uninstall_dir.is_some() {
            "uninstall"
        } else if installed.is_some() {
            "update"
        } else {
            "install"
        },
        fonts_active: uninstall_dir.as_ref().map(|_| registry::typeshelf_fonts()),
        dir: uninstall_dir.unwrap_or_else(install::default_dir),
        installed_version: installed.map(|i| i.version),
        installed_to: Mutex::default(),
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(|app| {
            let config = app.config().app.windows.first().ok_or("no window in tauri.conf.json")?.clone();
            // Keep WebView2's profile out of %LOCALAPPDATA%; it's deleted when Setup exits.
            tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?.data_directory(webview_dir()).build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![info, pick_dir, install, uninstall, close_app, launch_app, quit])
        .build(tauri::generate_context!())
        .expect("failed to start Glyphyard Setup")
        .run(move |_, event| {
            if let RunEvent::Exit = event {
                cleanup(&args, true);
            }
        });
}

/// `--from`, else the registered install, else the folder this uninstall.exe sits in.
fn find_install(args: &Args) -> PathBuf {
    let own = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    args.from
        .clone()
        .or_else(|| registry::installed().map(|i| i.dir))
        .or_else(|| own.filter(|d| install::looks_installed(d)))
        .unwrap_or_else(|| {
            log("uninstall: Glyphyard is not installed");
            if !args.silent {
                message_box("Glyphyard is not installed.", false);
            }
            exit(1)
        })
}

fn running_inside(dir: &Path) -> bool {
    let me = std::env::current_exe().and_then(fs::canonicalize);
    match (me, fs::canonicalize(dir)) {
        (Ok(me), Ok(dir)) => me.starts_with(dir),
        _ => false,
    }
}

/// A running exe can't delete its own folder, so uninstall from a copy in %TEMP%.
fn relaunch_from_temp(args: &Args, dir: &Path) -> io::Result<Child> {
    let copy = std::env::temp_dir().join(format!("typeshelf-uninstall-{}.exe", std::process::id()));
    fs::copy(std::env::current_exe()?, &copy)?;
    // Not inside `dir`: a working directory there would keep the folder from being deleted.
    Command::new(&copy).args(&args.raw).arg("--from").arg(dir).current_dir(std::env::temp_dir()).spawn()
}

fn webview_dir() -> PathBuf {
    std::env::temp_dir().join("typeshelf-setup-webview2")
}

fn cleanup(args: &Args, gui: bool) {
    let me = std::env::current_exe().ok().filter(|_| args.from.is_some());
    let webview = gui.then(webview_dir);
    let paths: Vec<&Path> = me.iter().chain(webview.iter()).map(PathBuf::as_path).collect();
    if !paths.is_empty() {
        process::delete_after_exit(&paths);
    }
}

fn webview2_missing() {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let text = "Glyphyard Setup needs the Microsoft Edge WebView2 Runtime, which isn't installed on this PC.\n\n\
                Open the download page now? Run Setup again once it's installed.";
    if message_box(text, true) {
        unsafe {
            ShellExecuteW(None, w!("open"), &HSTRING::from(WEBVIEW2_URL), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
        }
    }
}

/// Native message box (no WebView needed). With `ask`, Yes/No and returns true on Yes.
fn message_box(text: &str, ask: bool) -> bool {
    use windows::core::{w, HSTRING};
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONWARNING, MB_OK, MB_YESNO};
    let buttons = if ask { MB_YESNO } else { MB_OK };
    unsafe { MessageBoxW(None, &HSTRING::from(text), w!("Glyphyard Setup"), buttons | MB_ICONWARNING) == IDYES }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_after_cases() {
        assert!(launch_after(true, true, true, false, false));
        assert!(launch_after(false, true, true, true, false));
        assert!(!launch_after(false, true, false, true, false));
        assert!(!launch_after(false, false, true, true, false));
        assert!(!launch_after(false, true, true, true, true));
        assert!(!launch_after(false, true, true, false, false));
    }

    #[test]
    fn parse_reads_launch() {
        let a = parse(vec!["--silent".into(), "--launch".into()]);
        assert!(a.silent && a.launch);
        assert!(!parse(vec!["--silent".into()]).launch);
    }
}
