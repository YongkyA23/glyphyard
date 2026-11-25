//! Folder watching: debounced file events → incremental rescan of the affected folders.

use crate::{scan::folder_rows, Engine, EngineError, Result};
use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Duration;

pub(crate) type Watcher = Debouncer<RecommendedWatcher, RecommendedCache>;

impl Engine {
    /// Starts watching every online folder; folders added or removed later are followed.
    pub fn start_watching(self: &Arc<Self>) -> Result<()> {
        let weak: Weak<Engine> = Arc::downgrade(self);
        let mut debouncer = new_debouncer(Duration::from_secs(2), None, move |res: DebounceEventResult| {
            let (Some(engine), Ok(events)) = (weak.upgrade(), res) else { return };
            let paths: Vec<PathBuf> = events.iter().flat_map(|e| e.paths.iter().cloned()).collect();
            if !paths.is_empty() {
                let _ = engine.on_fs_change(&paths);
            }
        })
        .map_err(|e| EngineError::new("watch", e.to_string()))?;
        for f in folder_rows(&self.db())?.into_iter().filter(|f| f.online) {
            let _ = debouncer.watch(&f.path, RecursiveMode::Recursive);
        }
        *self.watcher.lock().unwrap() = Some(debouncer);
        Ok(())
    }

    pub fn stop_watching(&self) {
        self.watcher.lock().unwrap().take();
    }

    pub(crate) fn watch_folder(&self, path: &Path, on: bool) {
        if let Some(w) = self.watcher.lock().unwrap().as_mut() {
            let _ = if on { w.watch(path, RecursiveMode::Recursive) } else { w.unwatch(path) };
        }
    }

    fn on_fs_change(&self, paths: &[PathBuf]) -> Result<()> {
        let ids = self.folders_containing(paths)?;
        let changed = self.scan_folders(&ids)?;
        if changed {
            self.library_changed("watch");
        }
        Ok(())
    }
}
