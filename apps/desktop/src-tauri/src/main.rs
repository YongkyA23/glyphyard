// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adobe;
mod commands;
mod sync;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::async_runtime::JoinHandle;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, Window, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tokio::sync::oneshot;
use tracing_appender::non_blocking::WorkerGuard;
use typeshelf_adobe_bridge::BridgeHandle;
use typeshelf_engine::{Engine, EngineError, SystemFonts};
use typeshelf_sync::{CancellationToken, SyncClient};

/// Everything the commands and background tasks share (`app.state::<Ctx>()`).
pub struct Ctx {
    pub engine: Arc<Engine>,
    pub data_dir: PathBuf,
    /// The current server client and the token that stops its background loop.
    pub sync: Mutex<(Arc<SyncClient>, CancellationToken)>,
    /// Adobe pairing prompts waiting for `adobe_pair_respond`, by requestId.
    pub pairs: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    pub bridge: Mutex<Option<BridgeHandle>>,
    /// Settings › "Start in the tray": closing the main window only hides it.
    pub tray_mode: AtomicBool,
    events_task: Mutex<Option<JoinHandle<()>>>,
    log_guard: Mutex<Option<WorkerGuard>>,
}

impl Ctx {
    pub fn client(&self) -> Arc<SyncClient> {
        self.sync.lock().unwrap().0.clone()
    }

    /// Runs an engine call on the blocking pool (the engine is synchronous and holds a DB lock).
    pub async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Engine) -> Result<T, EngineError> + Send + 'static,
    ) -> Result<T, EngineError> {
        blocking(&self.engine, f).await
    }
}

pub async fn blocking<T: Send + 'static>(
    engine: &Arc<Engine>,
    f: impl FnOnce(&Engine) -> Result<T, EngineError> + Send + 'static,
) -> Result<T, EngineError> {
    let engine = engine.clone();
    tauri::async_runtime::spawn_blocking(move || f(&engine))
        .await
        .map_err(|e| EngineError::new("internal", e.to_string()))?
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn toggle_quick(app: &AppHandle) {
    let Some(w) = app.get_webview_window("quick") else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
    } else {
        let _ = w.center();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Swaps the quick-activate shortcut; on failure the old one stays registered.
pub fn set_shortcut(app: &AppHandle, old: &str, new: &str) -> Result<(), EngineError> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    gs.register(new).map_err(|e| {
        let _ = gs.register(old);
        EngineError::new("shortcut", format!("{new} can't be used as a shortcut: {e}"))
    })
}

pub fn set_autostart(app: &AppHandle, on: bool) -> Result<(), EngineError> {
    let al = app.autolaunch();
    let res = if on { al.enable() } else { al.disable() };
    res.map_err(|e| EngineError::new("autostart", e.to_string()))
}

fn init_logging(dir: &Path) -> Option<WorkerGuard> {
    use tracing_subscriber::fmt::writer::MakeWriterExt;
    std::fs::create_dir_all(dir).ok()?;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("typeshelf")
        .filename_suffix("log")
        .max_log_files(7)
        .build(dir)
        .ok()?;
    let (file, guard) = tracing_appender::non_blocking(appender);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false).with_writer(file.and(std::io::stderr)).init();
    Some(guard)
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let log_guard = init_logging(&app.path().app_log_dir()?);
    let data_dir = app.path().app_data_dir()?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), data_dir = %data_dir.display(), "starting");

    let engine = Arc::new(Engine::open(&data_dir, SystemFonts)?);
    if engine.get_connection()?.server_url.is_empty() {
        engine.set_connection(sync::DEFAULT_SERVER_URL, sync::DEFAULT_INSTALL_KEY)?;
    }
    let h = handle.clone();
    engine.on_ui_event(move |ev| {
        let _ = h.emit(ev.name(), ev.payload());
    });
    // The callback must not re-enter the engine, so events go through a channel to the outbox.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    engine.on_event(move |ev| {
        let _ = tx.send(ev);
    });

    let settings = engine.get_settings()?;
    let conn = engine.get_connection()?;
    let idle = sync::new_client(&data_dir, conn.server_url, conn.install_key, engine.token()?);
    app.manage(Ctx {
        engine: engine.clone(),
        data_dir: data_dir.clone(),
        sync: Mutex::new((idle, CancellationToken::new())),
        pairs: Mutex::default(),
        bridge: Mutex::default(),
        tray_mode: AtomicBool::new(settings.start_hidden),
        events_task: Mutex::new(Some(tauri::async_runtime::spawn(sync::drain_events(handle.clone(), rx)))),
        log_guard: Mutex::new(log_guard),
    });

    if !settings.start_hidden {
        show_main(&handle);
    }
    let open = MenuItem::with_id(app, "open", "Open Typeshelf", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().ok_or("no app icon")?)
        .tooltip("Typeshelf")
        .menu(&Menu::with_items(app, &[&open, &quit])?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;

    if let Err(e) = app.global_shortcut().register(settings.quick_shortcut.as_str()) {
        tracing::warn!("quick shortcut {} not registered: {e}", settings.quick_shortcut);
    }
    // ponytail: dev builds never touch the login items; the setting is applied on change and by release builds.
    if !cfg!(debug_assertions) {
        if let Err(e) = set_autostart(&handle, settings.start_at_login) {
            tracing::warn!("start at login: {e}");
        }
    }

    tauri::async_runtime::spawn(startup(handle.clone()));
    tauri::async_runtime::spawn(adobe::start(handle));
    Ok(())
}

/// open → rescan → repair → watch, then connect (the first connect uploads the snapshot).
async fn startup(app: AppHandle) {
    let engine = app.state::<Ctx>().engine.clone();
    let res = blocking(&engine, |e| {
        e.rescan()?;
        let health = e.repair_now()?;
        tracing::info!(active = health.active, removed = health.removed, reregistered = health.reregistered, "repair");
        Ok(())
    })
    .await;
    if let Err(e) = res {
        tracing::error!("startup scan/repair: {e}");
    }
    let watched = engine.clone();
    match tauri::async_runtime::spawn_blocking(move || watched.start_watching()).await {
        Ok(Err(e)) => tracing::error!("watching folders: {e}"),
        Err(e) => tracing::error!("watching folders: {e}"),
        Ok(Ok(())) => {}
    }
    let _ = app.emit("library-changed", serde_json::json!({ "reason": "scan" }));
    if let Err(e) = sync::restart(&app).await {
        tracing::error!("starting sync: {e}");
    }
}

fn on_window_event(window: &Window, event: &WindowEvent) {
    match (window.label(), event) {
        ("quick", WindowEvent::Focused(false)) => {
            let _ = window.hide();
        }
        // The popup is reused, so closing it (Esc in the UI) only hides it.
        ("quick", WindowEvent::CloseRequested { api, .. }) => {
            api.prevent_close();
            let _ = window.hide();
        }
        ("main", WindowEvent::CloseRequested { api, .. }) => {
            if window.state::<Ctx>().tray_mode.load(Ordering::Relaxed) {
                api.prevent_close();
                let _ = window.hide();
            } else {
                window.app_handle().exit(0);
            }
        }
        _ => {}
    }
}

fn shutdown(app: &AppHandle) {
    let ctx = app.state::<Ctx>();
    let engine = &ctx.engine;
    if engine.get_settings().is_ok_and(|s| s.deactivate_on_quit) {
        match engine.deactivate_all() {
            Ok(r) => tracing::info!("deactivated {} fonts on quit", r.iter().filter(|r| r.ok).count()),
            Err(e) => tracing::error!("deactivate on quit: {e}"),
        }
    }
    engine.stop_watching();
    // Dropping the callback closes the event channel; wait for the last events to reach the outbox.
    engine.on_event(|_| {});
    if let Some(task) = ctx.events_task.lock().unwrap().take() {
        let _ = tauri::async_runtime::block_on(async { tokio::time::timeout(Duration::from_secs(5), task).await });
    }
    ctx.sync.lock().unwrap().1.cancel();
    if let Some(bridge) = ctx.bridge.lock().unwrap().take() {
        bridge.shutdown();
    }
    tracing::info!("stopped");
    drop(ctx.log_guard.lock().unwrap().take());
}

/// The same login item tauri-plugin-autostart manages (name = productName, path = this exe, no args).
fn set_start_at_login(on: bool) -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?.display().to_string();
    let item = auto_launch::AutoLaunchBuilder::new().set_app_name("Typeshelf").set_app_path(&exe).build()?;
    if on {
        item.enable()?;
    } else {
        item.disable()?;
    }
    // ponytail: Windows-only (Setup is the only caller); Tauri's app_data_dir there is %APPDATA%\<identifier>.
    let data_dir = PathBuf::from(std::env::var_os("APPDATA").ok_or("APPDATA is not set")?).join("app.typeshelf.desktop");
    Engine::open(data_dir, SystemFonts)?.update_settings(serde_json::json!({ "startAtLogin": on }))?;
    Ok(())
}

fn main() {
    // Run by the uninstaller: remove every font registration Typeshelf made, so nothing stale is left behind.
    if std::env::args().any(|a| a == "--unregister-fonts") {
        typeshelf_activation::reconcile(&[]);
        return;
    }
    // Run by Setup: apply the "start at login" choice and remember it, so startup doesn't undo it.
    if let Some(on) = std::env::args().find_map(|a| a.strip_prefix("--set-start-at-login=").map(|v| v == "true")) {
        if let Err(e) = set_start_at_login(on) {
            eprintln!("--set-start-at-login: {e}");
            std::process::exit(1);
        }
        return;
    }
    tauri::Builder::default()
        // Must be the first plugin.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show_main(app)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, e| {
                    if e.state() == ShortcutState::Pressed {
                        toggle_quick(app);
                    }
                })
                .build(),
        )
        .setup(setup)
        .on_window_event(on_window_event)
        .invoke_handler(commands::handler())
        .build(tauri::generate_context!())
        .expect("failed to start Typeshelf")
        .run(|app, event| match event {
            RunEvent::Exit => shutdown(app),
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => show_main(app),
            _ => {}
        });
}
