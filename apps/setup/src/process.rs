//! Finding, stopping and starting Glyphyard, plus running helper programs without a console window.

use std::ffi::OsStr;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

pub const APP_EXE: &str = "typeshelf.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn hidden(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Marks "a Glyphyard Setup or uninstall is running" for the app, which then does not start a pending update when
/// it is closed (apps/desktop/src-tauri/src/update.rs, `SETUP_MUTEX`: same name). The handle is never closed;
/// Windows releases it when this process exits.
pub fn hold_setup_mutex() {
    use windows::core::w;
    use windows::Win32::System::Threading::CreateMutexW;
    // SAFETY: no security attributes, a static name; the returned handle is intentionally kept open.
    let _ = unsafe { CreateMutexW(None, false, w!("Local\\TypeshelfSetupRunning")) };
}

/// Any process named typeshelf.exe (it holds the files we are about to replace or delete).
pub fn app_running() -> bool {
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return false };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = false;
        let mut more = Process32FirstW(snap, &mut entry).is_ok();
        while more && !found {
            let name = &entry.szExeFile;
            let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            found = String::from_utf16_lossy(&name[..len]).eq_ignore_ascii_case(APP_EXE);
            more = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
        found
    }
}

/// taskkill without /F posts WM_CLOSE; closing the window only hides the app to the tray, so force after 5 s.
pub fn close_app() {
    let taskkill = |force: bool| {
        let mut cmd = hidden("taskkill");
        cmd.args(["/IM", APP_EXE]);
        if force {
            cmd.arg("/F");
        }
        let _ = cmd.output();
    };
    taskkill(false);
    if !wait_gone(Duration::from_secs(5)) {
        taskkill(true);
        wait_gone(Duration::from_secs(5));
    }
}

fn wait_gone(timeout: Duration) -> bool {
    let start = Instant::now();
    while app_running() {
        if start.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    true
}

/// Runs `<dir>\typeshelf.exe <arg>` and waits for it.
pub fn run_app(dir: &Path, arg: &str) -> io::Result<()> {
    let status = hidden(dir.join(APP_EXE)).arg(arg).current_dir(dir).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{APP_EXE} {arg} failed ({status})")))
    }
}

pub fn launch_app(dir: &Path) -> io::Result<()> {
    Command::new(dir.join(APP_EXE)).current_dir(dir).spawn().map(drop)
}

/// Deletes files/folders a few seconds after this process exits (a running exe can't delete itself).
pub fn delete_after_exit(paths: &[&Path]) {
    let mut script = String::from("/C ping -n 4 127.0.0.1 >NUL");
    for p in paths {
        let p = p.display();
        script.push_str(&format!(" & rmdir /S /Q \"{p}\" 2>NUL & del /F /Q \"{p}\" 2>NUL"));
    }
    let _ = hidden("cmd").raw_arg(script).spawn();
}
