use std::collections::HashSet;
use std::path::{Path, PathBuf};

use windows::core::HSTRING;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{AddFontResourceW, RemoveFontResourceW};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, HWND_BROADCAST, WM_FONTCHANGE};
use windows_registry::{Key, CURRENT_USER, LOCAL_MACHINE};

use crate::{ActivationError, LedgerEntry, ReconcileReport};

const FONTS_KEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Fonts";
const SUFFIX: &str = " (Typeshelf)";
const ERROR_SHARING_VIOLATION: i32 = 32;

fn os(e: impl std::fmt::Display) -> ActivationError {
    ActivationError::Os(e.to_string())
}

fn absolute(path: &Path) -> Result<PathBuf, ActivationError> {
    std::path::absolute(path).map_err(os)
}

/// Registry paths compare case-insensitively.
fn path_key(path: &str) -> String {
    path.to_lowercase()
}

fn value_name(entry: &LedgerEntry) -> String {
    // Control characters (FontBase left "...(FontBase)\n") make a name nobody can match again.
    let clean: String = entry.full_name.chars().filter(|c| !c.is_control()).collect();
    let name = match clean.trim() {
        "" => entry.path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
        n => n.to_string(),
    };
    format!("{name}{SUFFIX}")
}

fn fonts_key() -> Result<Key, ActivationError> {
    CURRENT_USER.create(FONTS_KEY).map_err(os)
}

/// Our values as (name, data). Non-string data comes back empty, so it reads as a missing file.
fn our_values(key: &Key) -> Result<Vec<(String, String)>, ActivationError> {
    Ok(key
        .values()
        .map_err(os)?
        .filter(|(name, _)| name.ends_with(SUFFIX))
        .map(|(name, v)| (name, String::try_from(v).unwrap_or_default()))
        .collect())
}

/// Drops every reference this session holds on the file (each AddFontResourceW adds one).
fn unload(path: &Path) {
    let wide = HSTRING::from(path.as_os_str());
    for _ in 0..64 {
        if !unsafe { RemoveFontResourceW(&wide) }.as_bool() {
            break;
        }
    }
}

pub fn activate(entry: &LedgerEntry) -> Result<(), ActivationError> {
    let path = absolute(&entry.path)?;
    match std::fs::File::open(&path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ActivationError::FileMissing(path))
        }
        Err(e) if e.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => {
            return Err(ActivationError::InUse(path))
        }
        Err(e) => return Err(os(e)),
    }
    if unsafe { AddFontResourceW(&HSTRING::from(path.as_os_str())) } == 0 {
        return Err(ActivationError::InvalidFont(path));
    }
    let written = fonts_key().and_then(|k| {
        k.set_string(value_name(entry), path.to_string_lossy())
            .map_err(os)
    });
    if written.is_err() {
        unload(&path);
    }
    written
}

pub fn deactivate(entry: &LedgerEntry) -> Result<(), ActivationError> {
    deactivate_many(std::slice::from_ref(entry)).pop().unwrap_or(Ok(()))
}

/// AddFontResourceW is slow (~40 ms per file) but scales across threads, so batches run on up to 8.
pub fn activate_many(entries: &[LedgerEntry]) -> Vec<Result<(), ActivationError>> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
    let chunk = entries.len().div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let workers: Vec<_> = entries.chunks(chunk).map(|c| s.spawn(move || c.iter().map(activate).collect::<Vec<_>>())).collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap_or_else(|_| vec![Err(ActivationError::Os("activation thread panicked".into()))]))
            .collect()
    })
}

/// Removes each registration by its value name; only entries stored under another name (renamed font,
/// shared full name) need a scan of the Fonts key, done once for the batch. Reading that key takes ~0.2 s,
/// so scanning it per font made turning off hundreds of fonts take minutes.
pub fn deactivate_many(entries: &[LedgerEntry]) -> Vec<Result<(), ActivationError>> {
    let key = match fonts_key() {
        Ok(k) => k,
        Err(e) => return entries.iter().map(|_| Err(os(&e))).collect(),
    };
    let mut out = Vec::with_capacity(entries.len());
    let mut unnamed = Vec::new(); // (index in out, path key)
    for e in entries {
        let path = match absolute(&e.path) {
            Ok(p) => p,
            Err(err) => {
                out.push(Err(err));
                continue;
            }
        };
        unload(&path);
        let target = path_key(&path.to_string_lossy());
        let name = value_name(e);
        match key.get_string(&name) {
            Ok(data) if path_key(&data) == target => out.push(key.remove_value(&name).map_err(os)),
            _ => {
                unnamed.push((out.len(), target));
                out.push(Ok(()));
            }
        }
    }
    if !unnamed.is_empty() {
        match our_values(&key) {
            Ok(values) => {
                for (i, target) in unnamed {
                    for (name, _) in values.iter().filter(|(_, d)| path_key(d) == target) {
                        if let Err(e) = key.remove_value(name) {
                            out[i] = Err(os(e));
                        }
                    }
                }
            }
            Err(e) => {
                for (i, _) in unnamed {
                    out[i] = Err(os(&e));
                }
            }
        }
    }
    out
}

pub fn reconcile(ledger: &[LedgerEntry]) -> ReconcileReport {
    let mut report = ReconcileReport::default();
    let Ok(key) = fonts_key() else { return report };
    let wanted: HashSet<String> = ledger
        .iter()
        .map(|e| path_key(&e.path.to_string_lossy()))
        .collect();
    let mut kept = HashSet::new();
    for (name, data) in our_values(&key).unwrap_or_default() {
        let k = path_key(&data);
        if wanted.contains(&k) && Path::new(&data).is_file() {
            kept.insert(k);
            continue;
        }
        unload(Path::new(&data));
        if key.remove_value(&name).is_ok() {
            report.removed.push(name);
        }
    }
    // ponytail: two ledger entries with the same full name share one value name and would
    // overwrite each other here; the engine keeps one file per PostScript name, so full names
    // are unique in practice.
    for e in ledger {
        if !e.path.is_file() {
            report.missing.push(e.path.clone());
        } else if !kept.contains(&path_key(&e.path.to_string_lossy())) && activate(e).is_ok() {
            report.reregistered.push(e.path.clone());
        }
    }
    if !report.removed.is_empty() || !report.reregistered.is_empty() {
        notify_font_change();
    }
    report
}

pub fn installed_ps_names() -> HashSet<String> {
    let windir = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
    let system_dir = PathBuf::from(windir).join("Fonts");
    let mut files = Vec::new();
    let mut ours = HashSet::new();
    for root in [LOCAL_MACHINE, CURRENT_USER] {
        let Ok(key) = root.open(FONTS_KEY) else { continue };
        let Ok(values) = key.values() else { continue };
        for (name, v) in values {
            let Ok(data) = String::try_from(v) else { continue };
            if name.ends_with(SUFFIX) {
                ours.insert(path_key(&data));
            } else {
                // Relative data (e.g. "arial.ttf") lives in %WINDIR%\Fonts; join keeps absolute paths.
                files.push(system_dir.join(data));
            }
        }
    }
    crate::walk(&system_dir, &mut files);
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        crate::walk(&PathBuf::from(local).join(r"Microsoft\Windows\Fonts"), &mut files);
    }
    let mut seen = HashSet::new();
    files.retain(|p| {
        let k = path_key(&p.to_string_lossy());
        !ours.contains(&k) && seen.insert(k)
    });
    crate::ps_names_of(&files)
}

pub fn notify_font_change() {
    // PostMessage, not SendMessage: a hung window must not block us.
    let _ = unsafe { PostMessageW(Some(HWND_BROADCAST), WM_FONTCHANGE, WPARAM(0), LPARAM(0)) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Reconcile touches every " (Typeshelf)" value, so tests in this crate must not overlap.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn unique() -> String {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        format!("{:x}{nanos:x}", std::process::id())
    }

    /// Removes exactly the values, font references and temp dir a test created, even if it panics.
    struct Cleanup {
        values: Vec<String>,
        fonts: Vec<PathBuf>,
        dir: PathBuf,
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.fonts.iter().for_each(|f| unload(f));
            if let Ok(key) = CURRENT_USER.create(FONTS_KEY) {
                for v in &self.values {
                    let _ = key.remove_value(v);
                }
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn setup(id: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("typeshelf-test-{id}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn copy_arial(dir: &Path) {
        let windir = PathBuf::from(std::env::var_os("WINDIR").unwrap());
        std::fs::copy(windir.join(r"Fonts\arial.ttf"), dir.join("arial-copy.ttf")).unwrap();
    }

    #[test]
    fn activate_then_deactivate() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let id = unique();
        let full_name = format!("Typeshelf Test {id}");
        let name = format!("{full_name} (Typeshelf)");
        let bad_name = format!("Typeshelf Test {id} Bad (Typeshelf)");
        let dir = setup(&id);
        let font = dir.join("arial-copy.ttf");
        let _cleanup = Cleanup {
            values: vec![name.clone(), bad_name.clone()],
            fonts: vec![font.clone()],
            dir: dir.clone(),
        };
        copy_arial(&dir);

        let entry = LedgerEntry { path: font.clone(), full_name };
        activate(&entry).unwrap();
        let key = CURRENT_USER.open(FONTS_KEY).unwrap();
        assert_eq!(key.get_string(&name).unwrap(), font.to_string_lossy());
        assert!(key.values().unwrap().any(|(n, _)| n == name), "value name must match exactly");

        deactivate(&entry).unwrap();
        assert!(key.get_string(&name).is_err());
        deactivate(&entry).unwrap();

        let missing = LedgerEntry { path: dir.join("nope.ttf"), full_name: "x".into() };
        assert!(matches!(activate(&missing), Err(ActivationError::FileMissing(_))));
        let bad = dir.join("bad.ttf");
        std::fs::write(&bad, b"not a font").unwrap();
        let bad_entry = LedgerEntry { path: bad, full_name: format!("Typeshelf Test {id} Bad") };
        assert!(matches!(activate(&bad_entry), Err(ActivationError::InvalidFont(_))));
        assert!(key.get_string(&bad_name).is_err());
    }

    #[test]
    fn reconcile_only_touches_our_values() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let id = unique();
        let stale = format!("Typeshelf Test {id} (Typeshelf)");
        let lost = format!("Typeshelf Test {id} Lost (Typeshelf)");
        let unrelated = format!("Typeshelf Unrelated Test {id}");
        let dir = setup(&id);
        let font = dir.join("arial-copy.ttf");
        let _cleanup = Cleanup {
            values: vec![stale.clone(), lost.clone(), unrelated.clone()],
            fonts: vec![font.clone()],
            dir: dir.clone(),
        };
        copy_arial(&dir);

        let key = CURRENT_USER.create(FONTS_KEY).unwrap();
        // Real Typeshelf registrations on this machine go in the ledger so reconcile keeps them.
        let mut ledger: Vec<LedgerEntry> = our_values(&key)
            .unwrap()
            .into_iter()
            .map(|(n, d)| LedgerEntry { path: d.into(), full_name: n.trim_end_matches(SUFFIX).into() })
            .collect();
        if ledger.iter().any(|e| !e.path.is_file()) {
            eprintln!("skipped: a stale real Typeshelf value exists and reconcile would remove it");
            return;
        }

        let gone = dir.join("gone.ttf");
        key.set_string(&stale, dir.join("stale.ttf").to_string_lossy()).unwrap();
        key.set_string(&unrelated, gone.to_string_lossy()).unwrap();
        ledger.push(LedgerEntry { path: font.clone(), full_name: format!("Typeshelf Test {id} Lost") });
        ledger.push(LedgerEntry { path: gone.clone(), full_name: "gone".into() });

        let report = reconcile(&ledger);
        assert!(report.removed.contains(&stale), "{report:?}");
        assert!(key.get_string(&stale).is_err());
        assert_eq!(key.get_string(&unrelated).unwrap(), gone.to_string_lossy());
        assert_eq!(report.reregistered, vec![font.clone()]);
        assert_eq!(key.get_string(&lost).unwrap(), font.to_string_lossy());
        assert_eq!(report.missing, vec![gone]);
    }

    #[test]
    fn finds_system_fonts() {
        assert!(installed_ps_names().contains("arialmt"));
    }
}
