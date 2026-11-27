//! The desktop app's backend as a plain synchronous library. `Engine` methods map 1:1 to the Tauri
//! commands in docs/desktop-commands.md; network calls (registration, name change, connection test)
//! live in typeshelf-sync and only use the local pieces stored here.

mod activation;
mod admin;
mod backup;
mod docfonts;
mod folders;
mod google;
mod library;
mod organize;
mod pairing;
mod pdf;
mod reconcile;
mod request;
mod scan;
mod search;
mod select_all;
mod settings;
mod stats;
mod store;
mod temporary;
mod types;
mod watch;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, RwLock};
use typeshelf_protocol::{Event, TimedEvent};

pub use docfonts::DocFonts;
pub use activation::{OsFonts, SystemFonts};
pub use pairing::AxisPreset;
pub use select_all::QueryTargets;
pub use stats::{Badge, FamilyUse, UnusedFamily, UsagePeriod, UsageStats};
pub use temporary::Until;
pub use types::*;
pub use typeshelf_activation::{ActivationError, LedgerEntry, ReconcileReport};

type Callback<T> = RwLock<Option<Box<dyn Fn(T) + Send + Sync>>>;

pub struct Engine {
    db: Mutex<rusqlite::Connection>,
    os: Box<dyn OsFonts>,
    data_dir: PathBuf,
    on_event: Callback<TimedEvent>,
    on_ui: Callback<UiEvent>,
    system_names: OnceLock<HashSet<String>>,
    last_plan: Mutex<Option<(bool, Vec<OrganizeChange>)>>,
    connection_state: Mutex<(ConnectionStatus, Option<String>)>,
    /// Held across one unlock attempt (read throttle, verify, write throttle).
    unlock_lock: Mutex<()>,
    watcher: Mutex<Option<watch::Watcher>>,
    google_catalog: Mutex<Option<std::sync::Arc<Vec<google::Meta>>>>,
    /// Set by `activate_in_background`: turning fonts on returns at once, the OS work runs on a thread.
    me: OnceLock<std::sync::Weak<Engine>>,
    /// The running background registration; the next one waits for it, so they apply in order.
    activation_job: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Engine {
    /// Opens (or creates) the store in `data_dir`, generates the install id on first run and checks
    /// which folders are online. Call [`Engine::rescan`] and [`Engine::repair_now`] after startup.
    pub fn open(data_dir: impl AsRef<Path>, os: impl OsFonts + 'static) -> Result<Engine> {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir)?;
        let conn = store::open(&data_dir.join("typeshelf.db"))?;
        let engine = Engine {
            db: Mutex::new(conn),
            os: Box::new(os),
            data_dir,
            on_event: RwLock::new(None),
            on_ui: RwLock::new(None),
            system_names: OnceLock::new(),
            last_plan: Mutex::new(None),
            connection_state: Mutex::new((ConnectionStatus::Offline, None)),
            unlock_lock: Mutex::new(()),
            watcher: Mutex::new(None),
            google_catalog: Mutex::new(None),
            me: OnceLock::new(),
            activation_job: Mutex::new(None),
        };
        engine.ensure_install_id()?;
        engine.refresh_folders()?;
        Ok(engine)
    }

    /// Server-bound events (font on/off, failures, organize, repair). The callback must not call
    /// back into the engine synchronously.
    pub fn on_event(&self, f: impl Fn(TimedEvent) + Send + Sync + 'static) {
        *self.on_event.write().unwrap() = Some(Box::new(f));
    }

    /// UI events; forward with `app.emit(name, payload)` where `name = UiEvent::name()`.
    pub fn on_ui_event(&self, f: impl Fn(UiEvent) + Send + Sync + 'static) {
        *self.on_ui.write().unwrap() = Some(Box::new(f));
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn db(&self) -> MutexGuard<'_, rusqlite::Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn emit(&self, event: Event) {
        if let Some(f) = self.on_event.read().unwrap().as_ref() {
            f(TimedEvent { at: chrono::Utc::now(), event });
        }
    }

    fn ui(&self, event: UiEvent) {
        if let Some(f) = self.on_ui.read().unwrap().as_ref() {
            f(event);
        }
    }

    fn library_changed(&self, reason: &'static str) {
        self.check_badges();
        self.ui(UiEvent::LibraryChanged { reason });
    }

    /// Lowercase PostScript names installed outside Glyphyard, read once per run.
    fn system_names(&self) -> &HashSet<String> {
        self.system_names.get_or_init(|| self.os.installed_ps_names())
    }
}
