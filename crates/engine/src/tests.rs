//! Logic tests against real font files copied from the OS, with an in-memory OS fake.

use super::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use typeshelf_protocol::{CommandKind, Event, FontRef, Source};

#[derive(Default)]
struct FakeState {
    registered: Mutex<HashSet<PathBuf>>,
    notifies: Mutex<u32>,
    /// activate() fails for these paths.
    fail: Mutex<HashSet<PathBuf>>,
    /// Held by a test to keep activate() waiting.
    gate: Mutex<()>,
    /// Held by a test to keep deactivate() and reconcile() waiting (a stalled OS call).
    stall: Mutex<()>,
    font_dirs: Mutex<Vec<PathBuf>>,
    /// Every successful activate() call, in order (a set cannot tell one call from several).
    activations: Mutex<Vec<PathBuf>>,
}

#[derive(Clone, Default)]
struct FakeOs(Arc<FakeState>);

impl OsFonts for FakeOs {
    fn activate(&self, e: &LedgerEntry) -> std::result::Result<(), ActivationError> {
        let _gate = self.0.gate.lock().unwrap();
        if self.0.fail.lock().unwrap().contains(&e.path) {
            return Err(ActivationError::InvalidFont(e.path.clone()));
        }
        if !e.path.is_file() {
            return Err(ActivationError::FileMissing(e.path.clone()));
        }
        self.0.registered.lock().unwrap().insert(e.path.clone());
        self.0.activations.lock().unwrap().push(e.path.clone());
        Ok(())
    }
    fn deactivate(&self, e: &LedgerEntry) -> std::result::Result<(), ActivationError> {
        let _stall = self.0.stall.lock().unwrap();
        self.0.registered.lock().unwrap().remove(&e.path);
        Ok(())
    }
    fn reconcile(&self, ledger: &[LedgerEntry]) -> ReconcileReport {
        let _stall = self.0.stall.lock().unwrap();
        let mut reg = self.0.registered.lock().unwrap();
        let want: HashSet<&PathBuf> = ledger.iter().map(|e| &e.path).collect();
        let mut report = ReconcileReport::default();
        reg.retain(|p| {
            let keep = want.contains(p) && p.is_file();
            if !keep {
                report.removed.push(p.display().to_string());
            }
            keep
        });
        for e in ledger {
            if !e.path.is_file() {
                report.missing.push(e.path.clone());
            } else if reg.insert(e.path.clone()) {
                report.reregistered.push(e.path.clone());
            }
        }
        report
    }
    fn installed_ps_names(&self) -> HashSet<String> {
        HashSet::new()
    }
    fn notify_font_change(&self) {
        *self.0.notifies.lock().unwrap() += 1;
    }
    fn font_dirs(&self) -> Vec<PathBuf> {
        self.0.font_dirs.lock().unwrap().clone()
    }
}

impl FakeOs {
    fn registered(&self) -> HashSet<PathBuf> {
        self.0.registered.lock().unwrap().clone()
    }
    /// How many times `path` was handed to activate().
    fn activations_of(&self, path: &Path) -> usize {
        self.0.activations.lock().unwrap().iter().filter(|p| p.as_path() == path).count()
    }
}

fn system_font(name: &str) -> Option<Vec<u8>> {
    std::fs::read(Path::new("C:\\Windows\\Fonts").join(name)).ok()
}

/// Rewrites the version digits in the name table (UTF-16BE and ASCII) to make "another version".
fn with_version(bytes: &[u8], to: &str) -> (Vec<u8>, String) {
    let from = typeshelf_core::parse_faces(bytes).unwrap()[0].version.clone();
    assert_eq!(from.len(), to.len());
    let mut out = bytes.to_vec();
    let utf16 = |s: &str| s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect::<Vec<u8>>();
    for (needle, repl) in [(utf16(&from), utf16(to)), (from.as_bytes().to_vec(), to.as_bytes().to_vec())] {
        let mut i = 0;
        while i + needle.len() <= out.len() {
            if out[i..i + needle.len()] == needle[..] {
                out[i..i + needle.len()].copy_from_slice(&repl);
                i += needle.len();
            } else {
                i += 1;
            }
        }
    }
    (out, from)
}

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
    os: FakeOs,
    engine: Engine,
    events: Arc<Mutex<Vec<Event>>>,
}

fn fixture() -> Option<Fixture> {
    system_font("arial.ttf")?;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("fonts");
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&root).unwrap();
    let os = FakeOs::default();
    let engine = Engine::open(&data, os.clone()).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    engine.on_event(move |e| sink.lock().unwrap().push(e.event));
    Some(Fixture { _tmp: tmp, root, data, os, engine, events })
}

impl Fixture {
    fn put(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let p = self.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn family(&self, name: &str) -> Family {
        let all = self.engine.list_families(&FamilyQuery::default()).unwrap();
        all.into_iter().find(|f| f.family == name).unwrap_or_else(|| panic!("family {name} not found"))
    }

    fn hashes(&self, name: &str) -> Vec<String> {
        let mut h: Vec<String> = self.family(name).faces.into_iter().map(|f| f.hash).collect();
        h.dedup();
        h
    }

    fn hashes_where(&self, filter: FamilyFilter) -> Vec<String> {
        let q = FamilyQuery { filter, ..Default::default() };
        self.engine.list_families(&q).unwrap().into_iter().flat_map(|f| f.faces).map(|f| f.hash).collect()
    }

    fn license_all(&self) {
        for f in self.engine.list_families(&FamilyQuery::default()).unwrap() {
            self.engine.set_license(&f.key, License::Licensed, None, None).unwrap();
        }
    }
}

macro_rules! fixture {
    () => {
        match fixture() {
            Some(f) => f,
            None => {
                eprintln!("system fonts not found, skipping");
                return;
            }
        }
    };
}

#[test]
fn scan_and_list_families() {
    let fx = fixture!();
    fx.put("a/arial.ttf", &system_font("arial.ttf").unwrap());
    if let Some(b) = system_font("arialbd.ttf") {
        fx.put("a/sub/arialbd.ttf", &b);
    }
    fx.put("a/notes.txt", b"ignored");
    let folders = fx.engine.add_folder(&fx.root).unwrap();
    assert_eq!(folders.len(), 1);
    assert!(folders[0].font_count >= 1);

    let arial = fx.family("Arial");
    assert!(arial.styles >= 1);
    assert_eq!(arial.active_state, ActiveState::None);
    assert_eq!(arial.license, License::Unknown);
    let json = serde_json::to_value(&arial).unwrap();
    assert!(json.get("activeState").is_some() && json.get("hasDuplicates").is_some());

    let detail = fx.engine.get_family(&arial.key).unwrap();
    assert!(detail.glyph_count > 100);
    assert!(!detail.features.is_empty());
    assert!(detail.files.iter().all(|f| f.status == FileStatus::Inactive));
    let json = serde_json::to_value(&detail).unwrap();
    assert_eq!(json["licenseInfo"]["label"], "unknown");
    assert!(json.get("key").is_some(), "Family fields are flattened");

    // Search, filters and incremental rescan.
    let q = FamilyQuery { search: Some("aria".into()), ..Default::default() };
    assert_eq!(fx.engine.list_families(&q).unwrap().len(), 1);
    let q = FamilyQuery { filter: FamilyFilter::Active, ..Default::default() };
    assert!(fx.engine.list_families(&q).unwrap().is_empty());
    fx.put("a/copy/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.rescan().unwrap();
    assert!(fx.family("Arial").has_duplicates);
    let q = FamilyQuery { filter: FamilyFilter::Duplicates, ..Default::default() };
    assert_eq!(fx.engine.list_families(&q).unwrap().len(), 1);
    assert!(!fx.engine.quick_search("arial").unwrap().is_empty());

    // Collections and tags.
    let cols = fx.engine.create_collection("Display", None).unwrap();
    let id = cols.iter().find(|c| c.name == "Display").unwrap().id;
    fx.engine.add_to_collection(id, std::slice::from_ref(&arial.key)).unwrap();
    let q = FamilyQuery { collection_id: Some(id), ..Default::default() };
    assert_eq!(fx.engine.list_families(&q).unwrap().len(), 1);
    let fav = cols.iter().find(|c| c.builtin == Some(Builtin::Favorites)).unwrap().id;
    assert_eq!(fx.engine.delete_collection(fav).unwrap_err().code, "builtin");
    assert_eq!(fx.engine.set_tags(&arial.key, &["sans".into(), " sans ".into()]).unwrap(), vec!["sans"]);
    assert_eq!(fx.engine.list_tags().unwrap()[0].count, 1);
}

#[test]
fn one_active_file_per_postscript_name() {
    let fx = fixture!();
    let bytes = system_font("arial.ttf").unwrap();
    let (old, _) = with_version(&bytes, "0.01");
    let new_path = fx.put("arial.ttf", &bytes);
    let old_path = fx.put("old/arial-old.ttf", &old);
    fx.engine.add_folder(&fx.root).unwrap();
    fx.license_all();

    let hashes = fx.hashes("Arial");
    assert_eq!(hashes.len(), 2, "two versions are two files");
    let results = fx.engine.set_active(&hashes, true).unwrap();
    assert_eq!(results.iter().filter(|r| r.ok).count(), 1);
    assert!(results.iter().any(|r| r.error == Some(ActivationErrorCode::NameConflict)));
    assert_eq!(fx.os.registered(), HashSet::from([new_path.clone()]), "newest version wins");

    // The user picks the old version: it replaces the new one.
    let old_hash = typeshelf_core::file_hash(&old);
    let r = fx.engine.set_active(std::slice::from_ref(&old_hash), true).unwrap();
    assert!(r[0].ok);
    assert_eq!(fx.os.registered(), HashSet::from([old_path]));
    let detail = fx.engine.get_family(&fx.family("Arial").key).unwrap();
    assert!(detail.files.iter().any(|f| f.status == FileStatus::Active));
    assert!(detail.files.iter().any(|f| f.status == FileStatus::OlderVersion) || detail.files.len() == 2);

    let r = fx.engine.set_active(&[old_hash], false).unwrap();
    assert!(r[0].ok);
    assert!(fx.os.registered().is_empty());
    assert!(*fx.os.0.notifies.lock().unwrap() >= 3);
}

#[test]
fn unlicensed_fonts_turn_on_without_asking() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let hashes = fx.hashes("Arial");
    assert_eq!(fx.family("Arial").license, License::Unknown);
    assert!(fx.engine.set_active(&hashes, true).unwrap()[0].ok);
    assert_eq!(fx.family("Arial").active_state, ActiveState::All);
    let recent = fx.engine.list_collections().unwrap().into_iter().find(|c| c.builtin == Some(Builtin::Recent)).unwrap();
    assert_eq!(recent.count, 1);
}

#[test]
fn project_set_off_keeps_fonts_needed_elsewhere() {
    let fx = fixture!();
    let Some(segoe) = system_font("segoeui.ttf").or_else(|| system_font("times.ttf")) else { return };
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.put("other.ttf", &segoe);
    fx.engine.add_folder(&fx.root).unwrap();
    fx.license_all();
    let fams = fx.engine.list_families(&FamilyQuery::default()).unwrap();
    let arial = fams.iter().find(|f| f.family == "Arial").unwrap().key.clone();
    let other = fams.iter().find(|f| f.family != "Arial").unwrap().key.clone();

    let sets = fx.engine.save_project_set(None, "Big", "", &[arial.clone(), other.clone()]).unwrap();
    let big = sets[0].id;
    let sets = fx.engine.save_project_set(None, "Small", "", std::slice::from_ref(&arial)).unwrap();
    let small = sets.iter().find(|s| s.name == "Small").unwrap().id;
    assert!(fx.engine.toggle_project_set(big, true).unwrap().iter().all(|r| r.ok));
    fx.engine.toggle_project_set(small, true).unwrap();
    assert_eq!(fx.os.registered().len(), 2);

    fx.engine.toggle_project_set(big, false).unwrap();
    let still: Vec<String> = fx
        .engine
        .list_families(&FamilyQuery { filter: FamilyFilter::Active, ..Default::default() })
        .unwrap()
        .into_iter()
        .map(|f| f.key)
        .collect();
    assert_eq!(still, vec![arial.clone()], "Arial is still needed by Small");
    assert!(fx.events.lock().unwrap().iter().any(|e| matches!(e, Event::ProjectSetToggled { on: false, .. })));

    let sets = fx.engine.list_project_sets().unwrap();
    assert!(!sets.iter().find(|s| s.id == big).unwrap().on);
    assert!(sets.iter().find(|s| s.id == small).unwrap().on);
    let json = serde_json::to_value(&sets[0]).unwrap();
    assert!(json.get("familyKeys").is_some() && json.get("lastUsed").is_some());
}

#[test]
fn offline_folder_survives_reopen() {
    let fx = fixture!();
    fx.put("lib/arial.ttf", &system_font("arial.ttf").unwrap());
    let lib = fx.root.join("lib");
    fx.engine.add_folder(&lib).unwrap();
    let key = fx.family("Arial").key;
    fx.engine.set_tags(&key, &["keep".into()]).unwrap();
    let hashes = fx.hashes("Arial");
    fx.engine.set_active(&hashes, true).unwrap();
    let Fixture { _tmp, root, data, os, engine, .. } = fx;
    drop(engine);

    // Moved far away: offline, data kept.
    let away = _tmp.path().join("elsewhere").join("deep").join("x");
    std::fs::create_dir_all(away.parent().unwrap()).unwrap();
    std::fs::rename(&lib, &away).unwrap();
    let engine = Engine::open(&data, os.clone()).unwrap();
    let folders = engine.list_folders().unwrap();
    assert_eq!(folders[0].status, FolderStatus::Offline);
    assert!(folders[0].offline_since.is_some());
    engine.rescan().unwrap();
    let fams = engine.list_families(&FamilyQuery::default()).unwrap();
    assert_eq!(fams.len(), 1, "offline fonts stay in the library");
    assert_eq!(engine.list_tags().unwrap()[0].name, "keep");
    assert_eq!(engine.health().unwrap().problems.len(), 1);

    // Back where it was: online again.
    std::fs::rename(&away, &lib).unwrap();
    engine.rescan().unwrap();
    assert_eq!(engine.list_folders().unwrap()[0].status, FolderStatus::Watching);
    drop(engine);

    // Renamed in place: found again by file id.
    let renamed = root.join("lib-renamed");
    std::fs::rename(&lib, &renamed).unwrap();
    let engine = Engine::open(&data, os.clone()).unwrap();
    let folders = engine.list_folders().unwrap();
    assert_eq!(folders[0].status, FolderStatus::Watching);
    assert_eq!(Path::new(&folders[0].path), renamed);
    assert!(os.registered().iter().all(|p| p.starts_with(&renamed)), "registration follows the folder");
    let health = engine.repair_now().unwrap();
    assert!(health.problems.is_empty());
    assert_eq!(health.active, 1);
}

#[test]
fn locate_folder_points_to_new_path() {
    let fx = fixture!();
    fx.put("lib/arial.ttf", &system_font("arial.ttf").unwrap());
    let lib = fx.root.join("lib");
    let id = fx.engine.add_folder(&lib).unwrap()[0].id;
    let away = fx.root.parent().unwrap().join("moved").join("a").join("b");
    std::fs::create_dir_all(away.parent().unwrap()).unwrap();
    std::fs::rename(&lib, &away).unwrap();
    fx.engine.rescan().unwrap();
    assert_eq!(fx.engine.list_folders().unwrap()[0].status, FolderStatus::Offline);
    let folders = fx.engine.locate_folder(id, &away).unwrap();
    assert_eq!(folders[0].status, FolderStatus::Watching);
    assert_eq!(folders[0].font_count, 1);
}

#[test]
fn settings_and_identity() {
    let fx = fixture!();
    let id = fx.engine.install_id().unwrap();
    assert_eq!(id.len(), 36);
    let s = fx.engine.update_settings(serde_json::json!({ "theme": "dark", "startHidden": true })).unwrap();
    assert_eq!(s.theme, Theme::Dark);
    assert!(s.start_hidden);
    assert!(fx.engine.update_settings(serde_json::json!({ "theme": "neon" })).is_err());
    let state = fx.engine.get_app_state().unwrap();
    assert!(!state.onboarded);
    assert_eq!(state.name_change_days, 30);
    fx.engine.set_identity("Ana", None).unwrap();
    fx.engine.set_token(Some("tok")).unwrap();
    fx.engine.set_connection("https://fonts.example", "key").unwrap();
    let state = fx.engine.get_app_state().unwrap();
    assert!(state.onboarded);
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(json["connection"]["installId"], id);
    assert_eq!(json["connection"]["serverUrl"], "https://fonts.example");
    drop(fx.engine);
    let reopened = Engine::open(&fx.data, FakeOs::default()).unwrap();
    assert_eq!(reopened.install_id().unwrap(), id, "install id is stable");
}

fn arial_ref(fx: &Fixture) -> FontRef {
    let f = &fx.family("Arial").faces[0];
    FontRef { hash: f.hash.clone(), ps_name: f.ps_name.clone(), family: f.family.clone(), style: f.style.clone() }
}

#[test]
fn organize_apply_and_undo_round_trip() {
    let fx = fixture!();
    let bytes = system_font("arial.ttf").unwrap();
    let (old, _) = with_version(&bytes, "0.01");
    let a = fx.put("a/arial.ttf", &bytes);
    let b = fx.put("b/arial.ttf", &bytes);
    let o = fx.put("old/arial.ttf", &old);
    let junk = fx.put("Thumbs.db", b"junk");
    fx.engine.add_folder(&fx.root).unwrap();
    fx.license_all();
    let new_hash = typeshelf_core::file_hash(&bytes);
    fx.engine.set_active(std::slice::from_ref(&new_hash), true).unwrap();

    let plan = fx.engine.organize_plan(false).unwrap();
    let cases: Vec<String> =
        plan.changes.iter().map(|c| serde_json::to_value(c).unwrap()["case"].as_str().unwrap().to_string()).collect();
    assert!(
        cases.contains(&"exact_copy".into()) && cases.contains(&"junk".into()) && cases.contains(&"different_version".into()),
        "{cases:?}"
    );
    assert_eq!(plan.scanned, 4);
    let json = serde_json::to_value(&plan.changes[0]).unwrap();
    assert!(json["action"]["op"].is_string() && json["id"].is_i64());
    let again = fx.engine.organize_plan(false).unwrap();
    let ids = |p: &OrganizePlan| p.changes.iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(ids(&plan), ids(&again), "ids are stable");

    let result = fx.engine.organize_apply(false, &[]).unwrap();
    assert_eq!(result.applied as usize, plan.changes.len());
    assert!(!junk.exists());
    assert_eq!([&a, &b, &o].iter().filter(|p| p.exists()).count(), 0, "copy trashed, versions renamed");
    let reg = fx.os.registered();
    assert_eq!(reg.len(), 1);
    let active_path = reg.iter().next().unwrap();
    assert!(active_path.is_file() && active_path.to_string_lossy().contains(" v"), "{active_path:?}");
    assert!(fx.family("Arial").faces.iter().any(|f| f.full_name.contains(" v")), "display override");
    assert!(fx.events.lock().unwrap().iter().any(|e| matches!(e, Event::OrganizeApplied { .. })));

    let history = fx.engine.undo_history(result.history_id).unwrap();
    assert!(history[0].undone);
    assert!(a.exists() && b.exists() && o.exists() && junk.exists());
    let reg = fx.os.registered();
    assert_eq!(reg.len(), 1);
    assert!(reg.iter().next().unwrap().is_file());
    assert!(fx.engine.repair_now().unwrap().problems.is_empty());
    assert!(fx.family("Arial").faces.iter().all(|f| !f.full_name.contains(" v")));
    assert_eq!(fx.engine.undo_history(result.history_id).unwrap_err().code, "already_undone");
}

#[test]
fn blocked_fonts_are_turned_off_and_refused() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let ui = Arc::new(Mutex::new(Vec::new()));
    let sink = ui.clone();
    fx.engine.on_ui_event(move |e| sink.lock().unwrap().push(e));
    let font = arial_ref(&fx);
    fx.engine.set_active(std::slice::from_ref(&font.hash), true).unwrap();

    fx.engine.apply_command(&CommandKind::Block { ps_name: font.ps_name.clone(), hash: None }).unwrap();
    assert!(fx.os.registered().is_empty());
    assert!(ui.lock().unwrap().iter().any(|e| matches!(e, UiEvent::AdminChange { action: "blocked", fonts } if fonts.len() == 1)));
    let r = fx.engine.set_active(std::slice::from_ref(&font.hash), true).unwrap();
    assert_eq!(r[0].error, Some(ActivationErrorCode::Blocked));
    assert!(fx.events.lock().unwrap().iter().any(|e| matches!(e, Event::ActivationFailed { reason, .. } if reason == "blocked")));

    fx.engine.apply_command(&CommandKind::Unblock { ps_name: font.ps_name.to_lowercase() }).unwrap();
    assert!(fx.engine.set_active(&[font.hash], true).unwrap()[0].ok);
}

#[test]
fn turning_off_an_admin_font_is_an_override() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let font = arial_ref(&fx);
    // Admin activation needs no license confirmation and may match by PostScript name only.
    let by_name = FontRef { hash: "unknown".into(), ..font.clone() };
    fx.engine.apply_command(&CommandKind::Activate { font: by_name }).unwrap();
    let fam = fx.family("Arial");
    assert!(fam.admin_set);
    assert_eq!(fam.faces[0].source, Some(Source::Admin));
    assert_eq!(fx.engine.snapshot().unwrap().active[0].source, Source::Admin);

    // A project set turning off leaves admin fonts alone.
    fx.engine.save_project_set(None, "S", "", std::slice::from_ref(&fam.key)).unwrap();
    let id = fx.engine.list_project_sets().unwrap()[0].id;
    fx.engine.toggle_project_set(id, true).unwrap();
    fx.engine.toggle_project_set(id, false).unwrap();
    assert_eq!(fx.os.registered().len(), 1);

    fx.engine.set_active(&[font.hash], false).unwrap();
    {
        let events = fx.events.lock().unwrap();
        assert!(events.iter().any(|e| matches!(e, Event::FontDeactivated { was_admin: true, source: Source::User, .. })));
        assert!(events.iter().any(|e| matches!(e, Event::FontActivated { source: Source::Admin, .. })));
    }

    let missing = FontRef { hash: "nope".into(), ps_name: "Nope-Regular".into(), family: "Nope".into(), style: "Regular".into() };
    assert_eq!(fx.engine.apply_command(&CommandKind::Activate { font: missing }).unwrap_err().code, "file_missing");
}

#[test]
fn delivery_saves_into_shared_library() {
    let fx = fixture!();
    let bytes = system_font("arial.ttf").unwrap();
    let file = typeshelf_protocol::LibraryFile {
        hash: typeshelf_core::file_hash(&bytes),
        filename: "arial.ttf".into(),
        size: bytes.len() as u64,
        ps_name: "ArialMT".into(),
        family: "Arial".into(),
        license: "licensed".into(),
        blocked: false,
    };
    let cmd = CommandKind::Deliver { file: file.clone(), activate: true };
    assert_eq!(fx.engine.apply_command(&cmd).unwrap_err().code, "download_required");
    assert_eq!(fx.engine.deliver(&file, b"tampered", true).unwrap_err().code, "hash_mismatch");
    fx.engine.deliver(&file, &bytes, true).unwrap();
    let shared = std::path::absolute(fx.data.join("Shared library").join("arial.ttf")).unwrap();
    assert!(shared.is_file());
    assert_eq!(fx.os.registered(), HashSet::from([shared]));
    let fam = fx.family("Arial");
    assert_eq!(fam.license, License::Licensed);
    assert_eq!(fam.faces[0].source, Some(Source::Library));
    // Already present: applying the command again just works.
    fx.engine.apply_command(&cmd).unwrap();

    fx.engine.apply_command(&CommandKind::OrganizeRequest).unwrap();
    let policy = typeshelf_protocol::Policy { name_change_days: 7, allow_connection_edit: false, notify_admin_changes: true };
    fx.engine.apply_command(&CommandKind::PolicyChanged { policy }).unwrap();
    assert_eq!(fx.engine.get_app_state().unwrap().name_change_days, 7);
    assert!(!fx.engine.get_connection().unwrap().editable);
}

fn write_pdf(path: &Path, embedded: Option<&[u8]>) {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut arial = dictionary! { "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "ABCDEF+ArialMT" };
    if let Some(bytes) = embedded {
        let file = doc.add_object(Stream::new(dictionary! {}, bytes.to_vec()));
        let desc = doc.add_object(dictionary! { "Type" => "FontDescriptor", "FontName" => "ABCDEF+ArialMT", "FontFile2" => file });
        arial.set("FontDescriptor", desc);
    }
    let arial = doc.add_object(arial);
    let other = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "QWERTY+Nope-Bold" });
    let one = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => arial, "F2" => other } });
    let two = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => arial } });
    let content = doc.add_object(Stream::new(dictionary! {}, b"BT /F1 12 Tf (Hi) Tj ET".to_vec()));
    let kids: Vec<Object> = [one, two]
        .into_iter()
        .map(|res| doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content, "Resources" => res }).into())
        .collect();
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 2 }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

#[test]
fn pdf_fonts_match_by_postscript_name() {
    let fx = fixture!();
    let bytes = system_font("arial.ttf").unwrap();
    let (old, _) = with_version(&bytes, "0.01");
    fx.put("old.ttf", &old);
    fx.engine.add_folder(&fx.root).unwrap();
    let pdf = fx.root.parent().unwrap().join("doc.pdf");
    write_pdf(&pdf, Some(&bytes));

    let fonts = fx.engine.pdf_fonts(&pdf).unwrap();
    let arial = fonts.iter().find(|f| f.ps_name == "ArialMT").expect("subset prefix stripped");
    assert_eq!(arial.pages, 2);
    assert_eq!(arial.status, PdfFontStatus::Version, "{}", arial.detail);
    let nope = fonts.iter().find(|f| f.ps_name == "Nope-Bold").unwrap();
    assert_eq!((nope.status, nope.pages, nope.hash.is_none()), (PdfFontStatus::Missing, 1, true));

    fx.put("new.ttf", &bytes);
    fx.engine.rescan().unwrap();
    let find = |fx: &Fixture| fx.engine.pdf_fonts(&pdf).unwrap().into_iter().find(|f| f.ps_name == "ArialMT").unwrap();
    let arial = find(&fx);
    assert_eq!(arial.status, PdfFontStatus::Inactive);
    let hash = arial.hash.unwrap();
    assert_eq!(hash, typeshelf_core::file_hash(&bytes));
    fx.engine.set_active(&[hash], true).unwrap();
    let arial = find(&fx);
    assert_eq!(arial.status, PdfFontStatus::Active);
    assert_eq!(serde_json::to_value(&arial).unwrap()["psName"], "ArialMT");
    assert_eq!(pdf::strip_subset("Arial"), "Arial");
    assert_eq!(pdf::strip_subset("abcdef+Arial"), "abcdef+Arial");
}

#[test]
fn backup_restore_and_undo() {
    let fx = fixture!();
    let key = "arial|tmc".to_string();
    let cols = fx.engine.create_collection("Client A", None).unwrap();
    let id = cols.iter().find(|c| c.name == "Client A").unwrap().id;
    fx.engine.add_to_collection(id, std::slice::from_ref(&key)).unwrap();
    fx.engine.set_tags(&key, &["sans".into()]).unwrap();
    fx.engine.set_license(&key, License::Client, Some("C:/lic.pdf"), Some("job 42")).unwrap();
    fx.engine.save_project_set(None, "Poster", "spring", std::slice::from_ref(&key)).unwrap();
    fx.engine.update_settings(serde_json::json!({ "sampleText": "Hamburgefonstiv" })).unwrap();
    let file = fx.data.join("backup.typeshelf");
    fx.engine.backup_create(&file).unwrap();

    fx.engine.delete_collection(id).unwrap();
    fx.engine.set_tags(&key, &[]).unwrap();
    fx.engine.update_settings(serde_json::json!({ "sampleText": "changed" })).unwrap();
    fx.engine.backup_restore(&file).unwrap();
    let cols = fx.engine.list_collections().unwrap();
    assert_eq!(cols.iter().find(|c| c.name == "Client A").unwrap().count, 1);
    assert_eq!(cols.iter().filter(|c| c.builtin.is_some()).count(), 2);
    assert_eq!(fx.engine.list_tags().unwrap()[0].name, "sans");
    assert_eq!(fx.engine.list_project_sets().unwrap()[0].family_keys, vec![key.clone()]);
    assert_eq!(fx.engine.get_settings().unwrap().sample_text, "Hamburgefonstiv");

    let history = fx.engine.list_history().unwrap();
    assert_eq!(history[0].kind, "restore");
    fx.engine.undo_history(history[0].id).unwrap();
    assert!(fx.engine.list_collections().unwrap().iter().all(|c| c.name != "Client A"));
    assert!(fx.engine.list_tags().unwrap().is_empty());
    assert_eq!(fx.engine.get_settings().unwrap().sample_text, "changed");
}

#[test]
fn ui_events_serialize_for_tauri() {
    let e = UiEvent::LibraryChanged { reason: "scan" };
    assert_eq!(e.name(), "library-changed");
    assert_eq!(e.payload(), serde_json::json!({ "reason": "scan" }));
    assert_eq!(UiEvent::OrganizeRequested {}.payload(), serde_json::json!({}));
    let err = EngineError::new("name_locked", "later").with("nextNameChange", "2026-11-01");
    assert_eq!(
        serde_json::to_value(&err).unwrap(),
        serde_json::json!({ "code": "name_locked", "message": "later", "nextNameChange": "2026-11-01" })
    );
}

#[test]
fn watcher_picks_up_new_files() {
    let Some(fx) = fixture() else { return };
    let Fixture { engine, root, _tmp, .. } = fx;
    let engine = Arc::new(engine);
    engine.add_folder(&root).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = Mutex::new(tx);
    engine.on_ui_event(move |e| {
        if e == (UiEvent::LibraryChanged { reason: "watch" }) {
            let _ = tx.lock().unwrap().send(());
        }
    });
    engine.start_watching().unwrap();
    std::fs::write(root.join("arial.ttf"), system_font("arial.ttf").unwrap()).unwrap();
    rx.recv_timeout(std::time::Duration::from_secs(20)).expect("library-changed from the watcher");
    assert_eq!(engine.list_families(&FamilyQuery::default()).unwrap().len(), 1);
    engine.stop_watching();
}


#[test]
fn google_catalog_pages_a_to_z_from_fresh_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    // A fresh cache file must be used as-is: no network in tests.
    std::fs::write(
        data.join("google-fonts.json"),
        r#"{"familyMetadataList":[
          {"family":"Lora","category":"Serif"},
          {"family":"Roboto","category":"Sans Serif"},
          {"family":"Zilla Slab","category":"Serif"},
          {"family":"inter","category":"Sans Serif"}]}"#,
    )
    .unwrap();
    let engine = Engine::open(&data, FakeOs::default()).unwrap();
    let names = |p: &GooglePage| p.items.iter().map(|f| f.family.clone()).collect::<Vec<_>>();

    // A–Z, ignoring case.
    let first = engine.google_catalog(None, None, 0, 2).unwrap();
    assert_eq!((names(&first), first.total), (vec!["inter".to_string(), "Lora".to_string()], 4));
    let second = engine.google_catalog(None, None, 2, 2).unwrap();
    assert_eq!(names(&second), ["Roboto", "Zilla Slab"]);
    assert!(engine.google_catalog(None, None, 4, 2).unwrap().items.is_empty());

    let serif = engine.google_catalog(Some("LO"), Some("Serif"), 0, 30).unwrap();
    assert_eq!((names(&serif), serif.total), (vec!["Lora".to_string()], 1));
}

#[test]
fn system_fonts_are_listed_read_only() {
    let fx = fixture!();
    let sys = fx.root.parent().unwrap().join("os-fonts");
    std::fs::create_dir_all(&sys).unwrap();
    std::fs::write(sys.join("arial.ttf"), system_font("arial.ttf").unwrap()).unwrap();
    *fx.os.0.font_dirs.lock().unwrap() = vec![sys.clone()];
    assert!(fx.engine.rescan().unwrap().is_empty(), "system folders stay out of the folder list");

    // Not in the library view, but listed (always on) under the System filter.
    assert!(fx.engine.list_families(&FamilyQuery::default()).unwrap().is_empty());
    let q = FamilyQuery { filter: FamilyFilter::System, ..Default::default() };
    let sys_fams = fx.engine.list_families(&q).unwrap();
    assert_eq!(sys_fams.len(), 1);
    let arial = &sys_fams[0];
    assert!(arial.system && arial.active_state == ActiveState::All);
    assert!(fx.engine.get_family(&arial.key).unwrap().files.iter().all(|f| f.status == FileStatus::System));

    // Turning it on or off never registers or touches the OS copy.
    let hashes: Vec<String> = arial.faces.iter().map(|f| f.hash.clone()).collect();
    assert!(fx.engine.set_active(&hashes, true).unwrap().iter().all(|r| r.ok));
    assert!(fx.engine.set_active(&hashes, false).unwrap().iter().all(|r| r.ok));
    assert!(fx.os.registered().is_empty());
    assert!(fx.engine.organize_plan(false).unwrap().changes.is_empty());
    assert!(sys.join("arial.ttf").is_file());

    // A library copy of the same font shows up in the library and can still be turned on.
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let lib = fx.family("Arial");
    assert!(!lib.system);
}

#[test]
fn folder_tree_and_folder_activation() {
    let fx = fixture!();
    let Some(bold) = system_font("arialbd.ttf") else { return };
    fx.put("a/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.put("a/b/arialbd.ttf", &bold);
    fx.engine.add_folder(&fx.root).unwrap();
    fx.license_all();

    let tree = fx.engine.folder_tree().unwrap();
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].font_count, 2);
    let a = &tree[0].children[0];
    assert_eq!((a.name.as_str(), a.font_count), ("a", 2));
    let b = &a.children[0];
    assert_eq!((b.name.as_str(), b.font_count, b.active_count), ("b", 1, 0));

    // A subfolder on/off touches only its own fonts.
    fx.engine.set_folder_active(&b.path, true).unwrap();
    assert_eq!(fx.os.registered().len(), 1);
    assert_eq!(fx.engine.folder_tree().unwrap()[0].children[0].active_count, 1);
    fx.engine.set_folder_active(&a.path, false).unwrap();
    assert!(fx.os.registered().is_empty());

    // Paging and the path filter.
    let q = FamilyQuery { path: Some(b.path.clone()), ..Default::default() };
    let page = fx.engine.family_page(&q, 0, 10).unwrap();
    assert_eq!(page.total, 1);
    assert!(fx.engine.family_page(&FamilyQuery::default(), 5, 10).unwrap().items.is_empty());
    assert_eq!(fx.engine.library_counts().unwrap().library, 1);

    // Auto-activate turns on what is there and what arrives later (picked up by a rescan).
    fx.engine.set_folder_auto(&b.path, true).unwrap();
    assert_eq!(fx.os.registered().len(), 1);
    let Some(italic) = system_font("ariali.ttf") else { return };
    fx.put("a/b/ariali.ttf", &italic);
    fx.put("a/ignored-italic.ttf", &system_font("arialbi.ttf").unwrap_or_default());
    fx.engine.rescan().unwrap();
    assert_eq!(fx.os.registered().len(), 2, "only the new file inside the auto folder is turned on");
    assert!(fx.engine.folder_tree().unwrap()[0].children[0].children[0].auto_activate);
}

/// Puts arial/times/consola/verdana (those present) in the library; false when one of the first three is missing.
fn put_mixed(fx: &Fixture) -> bool {
    for (rel, file) in [("sans/arial.ttf", "arial.ttf"), ("serif/times.ttf", "times.ttf"), ("mono/consola.ttf", "consola.ttf"), ("sans/verdana.ttf", "verdana.ttf")] {
        match system_font(file) {
            Some(b) => {
                fx.put(rel, &b);
            }
            None if file != "verdana.ttf" => return false,
            None => {}
        }
    }
    fx.engine.add_folder(&fx.root).unwrap();
    true
}

#[test]
fn categories_similar_and_random_order() {
    let fx = fixture!();
    if !put_mixed(&fx) {
        return;
    }
    assert_eq!(fx.family("Arial").category, Some(Category::Sans));
    assert_eq!(fx.family("Times New Roman").category, Some(Category::Serif));
    assert_eq!(fx.family("Consolas").category, Some(Category::Mono));
    let q = FamilyQuery { category: Some(Category::Mono), ..Default::default() };
    let mono: Vec<String> = fx.engine.list_families(&q).unwrap().into_iter().map(|f| f.family).collect();
    assert_eq!(mono, vec!["Consolas"]);
    assert_eq!(serde_json::to_value(fx.family("Arial")).unwrap()["category"], "sans");

    let arial = fx.family("Arial");
    let similar = fx.engine.similar_families(&arial.key, 10).unwrap();
    assert!(!similar.iter().any(|f| f.key == arial.key));
    assert_ne!(similar[0].family, "Consolas");
    if system_font("verdana.ttf").is_some() {
        assert_eq!(similar[0].family, "Verdana");
    }

    let order = |seed: f64| -> Vec<String> {
        let q = FamilyQuery { sort: SortBy::Random, seed: Some(seed), ..Default::default() };
        fx.engine.list_families(&q).unwrap().into_iter().map(|f| f.key).collect()
    };
    assert_eq!(order(0.42), order(0.42), "same seed, same order");
    let mut a = order(0.42);
    let mut b = order(7.0);
    a.sort();
    b.sort();
    assert_eq!(a, b);
}

#[test]
fn hidden_folders_leave_lists() {
    let fx = fixture!();
    if !put_mixed(&fx) {
        return;
    }
    let before = fx.engine.library_counts().unwrap().library;
    let serif = fx.root.join("serif").to_string_lossy().into_owned();
    let tree = fx.engine.set_folder_hidden(&serif, true).unwrap();
    assert!(tree[0].children.iter().any(|c| c.name == "serif" && c.hidden));
    let names = |q: FamilyQuery| -> Vec<String> { fx.engine.list_families(&q).unwrap().into_iter().map(|f| f.family).collect() };
    let times = "Times New Roman".to_string();
    assert!(!names(FamilyQuery::default()).contains(&times));
    assert!(names(FamilyQuery { include_hidden: true, ..Default::default() }).contains(&times));
    assert_eq!(names(FamilyQuery { path: Some(serif.clone()), ..Default::default() }), vec![times]);
    assert_eq!(fx.engine.library_counts().unwrap().library, before - 1);
    fx.engine.set_folder_hidden(&serif, false).unwrap();
    assert_eq!(fx.engine.library_counts().unwrap().library, before);
}

#[test]
fn family_override_survives_reparse() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    fx.license_all();
    let arial = fx.family("Arial");
    let cols = fx.engine.create_collection("Picks", None).unwrap();
    let id = cols.iter().find(|c| c.name == "Picks").unwrap().id;
    fx.engine.add_to_collection(id, std::slice::from_ref(&arial.key)).unwrap();
    let hashes = fx.hashes("Arial");

    fx.engine.set_family_name(&hashes, Some("My Sans")).unwrap();
    let mine = fx.family("My Sans");
    assert_eq!(mine.license, License::Licensed, "a family moved as a whole keeps its license");
    let q = FamilyQuery { collection_id: Some(id), ..Default::default() };
    assert_eq!(fx.engine.list_families(&q).unwrap()[0].family, "My Sans");
    assert_eq!(fx.engine.quick_search("my sans").unwrap().len(), 1);

    // A forced re-read (as after an upgrade) keeps the override.
    fx.engine.db().execute("UPDATE files SET mtime = -1", []).unwrap();
    fx.engine.rescan().unwrap();
    assert_eq!(fx.family("My Sans").key, mine.key);
    assert!(fx.engine.set_family_name(&hashes, Some("  ")).is_err());

    fx.engine.set_family_name(&hashes, None).unwrap();
    assert_eq!(fx.family("Arial").key, arial.key);
    assert_eq!(fx.engine.list_families(&q).unwrap()[0].family, "Arial");
}

#[test]
fn reparse_does_not_count_as_arrival() {
    let fx = fixture!();
    fx.put("a/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let a = fx.root.join("a").to_string_lossy().into_owned();
    fx.engine.set_folder_auto(&a, true).unwrap();
    fx.engine.set_folder_active(&a, false).unwrap();
    fx.engine.db().execute("UPDATE files SET mtime = -1", []).unwrap();
    fx.engine.rescan().unwrap();
    assert!(fx.os.registered().is_empty(), "same bytes re-read: not turned back on");
}

#[test]
fn import_paths_copies_into_imported_fonts() {
    let fx = fixture!();
    let Some(times) = system_font("times.ttf") else { return };
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let drop = fx.root.parent().unwrap().join("drop");
    std::fs::create_dir_all(drop.join("Kit/sub")).unwrap();
    std::fs::write(drop.join("Kit/sub/times.ttf"), &times).unwrap();
    std::fs::write(drop.join("Kit/arial-copy.ttf"), system_font("arial.ttf").unwrap()).unwrap();
    std::fs::write(drop.join("Kit/readme.txt"), b"x").unwrap();
    std::fs::write(drop.join("loose.ttf"), &times).unwrap();

    let paths = [drop.join("Kit").to_string_lossy().into_owned(), drop.join("loose.ttf").to_string_lossy().into_owned()];
    let res = fx.engine.import_paths(&paths).unwrap();
    assert_eq!((res.imported, res.skipped), (1, 2), "arial is already in the library; loose.ttf has the same bytes as times");
    let folder = PathBuf::from(&res.folder);
    assert!(folder.join("Kit/sub/times.ttf").is_file());
    fx.family("Times New Roman");
    assert!(fx.engine.folder_tree().unwrap().iter().any(|n| Path::new(&n.path) == folder));

    // Nothing is overwritten: a different file with the same name gets " (2)".
    let (v2, _) = with_version(&times, "9.99");
    std::fs::write(drop.join("Kit/sub/times.ttf"), &v2).unwrap();
    let res = fx.engine.import_paths(&paths[..1]).unwrap();
    assert_eq!((res.imported, res.skipped), (1, 1));
    assert!(folder.join("Kit/sub/times (2).ttf").is_file());
}

#[test]
fn delete_fonts_and_folders() {
    let fx = fixture!();
    if !put_mixed(&fx) {
        return;
    }
    fx.license_all();
    let arial = fx.hashes("Arial");
    fx.engine.set_active(&arial, true).unwrap();
    let res = fx.engine.delete_fonts(&arial).unwrap();
    assert_eq!((res.trashed, res.failed.len()), (1, 0));
    assert!(fx.os.registered().is_empty());
    assert!(!fx.root.join("sans/arial.ttf").exists());
    assert!(!fx.engine.list_families(&FamilyQuery::default()).unwrap().iter().any(|f| f.family == "Arial"));

    // A subfolder: turned off, trashed, root rescanned.
    let serif = fx.root.join("serif");
    fx.engine.set_folder_active(&serif.to_string_lossy(), true).unwrap();
    let res = fx.engine.delete_folder(&serif.to_string_lossy()).unwrap();
    assert_eq!(res.trashed, 1);
    assert!(!serif.exists() && fx.os.registered().is_empty());
    assert!(!fx.engine.list_families(&FamilyQuery::default()).unwrap().iter().any(|f| f.family == "Times New Roman"));

    // The watched root goes from the library too; system folders are refused and never touched.
    fx.engine.delete_folder(&fx.root.to_string_lossy()).unwrap();
    assert!(fx.engine.list_folders().unwrap().is_empty() && !fx.root.exists());
    let sys = fx.data.join("os-fonts");
    std::fs::create_dir_all(&sys).unwrap();
    std::fs::write(sys.join("arial.ttf"), system_font("arial.ttf").unwrap()).unwrap();
    *fx.os.0.font_dirs.lock().unwrap() = vec![sys.clone()];
    fx.engine.rescan().unwrap();
    assert_eq!(fx.engine.delete_folder(&sys.to_string_lossy()).unwrap_err().code, "not_allowed");
    assert_eq!(fx.engine.delete_fonts(&fx.hashes_where(FamilyFilter::System)).unwrap().trashed, 0);
    assert!(sys.join("arial.ttf").is_file());
}

#[test]
fn collection_activate_and_export() {
    let fx = fixture!();
    if !put_mixed(&fx) {
        return;
    }
    fx.license_all();
    let cols = fx.engine.create_collection("Parent", None).unwrap();
    let parent = cols.iter().find(|c| c.name == "Parent").unwrap().id;
    let cols = fx.engine.create_collection("Child", Some(parent)).unwrap();
    let child = cols.iter().find(|c| c.name == "Child").unwrap().id;
    fx.engine.add_to_collection(parent, &[fx.family("Arial").key]).unwrap();
    fx.engine.add_to_collection(child, &[fx.family("Consolas").key]).unwrap();

    let res = fx.engine.set_collection_active(parent, true).unwrap();
    assert!(res.iter().all(|r| r.ok));
    assert_eq!(fx.os.registered().len(), 2, "the sub-collection's family too");
    fx.engine.set_collection_active(parent, false).unwrap();
    assert!(fx.os.registered().is_empty());

    let dest = fx.data.join("out.zip");
    let res = fx.engine.export_collection(parent, &dest.to_string_lossy()).unwrap();
    assert_eq!(res.files, 2);
    let zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    let mut names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
    names.sort();
    assert_eq!(names, vec!["arial.ttf", "consola.ttf"]);
    assert_eq!(fx.engine.export_collection(9999, &dest.to_string_lossy()).unwrap_err().code, "not_found");
}

#[test]
fn glyph_search_and_supersearch() {
    let fx = fixture!();
    if !put_mixed(&fx) {
        return;
    }
    let names = |q: FamilyQuery| -> Vec<String> { fx.engine.list_families(&q).unwrap().into_iter().map(|f| f.family).collect() };
    let chars = |c: &str| names(FamilyQuery { chars: Some(c.into()), ..Default::default() });
    let search = |s: &str| names(FamilyQuery { search: Some(s.into()), ..Default::default() });
    let all = names(FamilyQuery::default());
    assert_eq!(chars("A é\n"), all, "whitespace ignored, Latin everywhere");
    assert!(chars("中").is_empty());
    let hebrew = chars("א");
    assert!(hebrew.contains(&"Arial".into()) && !hebrew.contains(&"Consolas".into()));
    assert_eq!(search("char:א"), hebrew);
    assert_eq!(search("glyph:中"), Vec::<String>::new());

    let arial = fx.family("Arial");
    fx.engine.set_tags(&arial.key, &["Client".into()]).unwrap();
    fx.engine.set_license(&arial.key, License::Licensed, None, None).unwrap();
    assert_eq!(search("arial"), vec!["Arial"], "plain words as before");
    assert_eq!(search("tag:client"), vec!["Arial"]);
    assert_eq!(search("license:licensed"), vec!["Arial"]);
    assert_eq!(search("category:mono"), vec!["Consolas"]);
    assert_eq!(search(r#"foundry:"tmc" category:serif"#), vec!["Times New Roman"]);
    assert_eq!(search("times category:sans"), Vec::<String>::new(), "tokens AND with words");
    assert_eq!(search("weight:400"), all);
    assert!(search("weight:>600").is_empty() && search("italic").is_empty() && search("active").is_empty());
    assert_eq!(search("inactive"), all);
    fx.engine.set_active(&fx.hashes("Arial"), true).unwrap();
    assert_eq!(search("active"), vec!["Arial"]);
    assert_eq!(search("nosuchkey:x"), Vec::<String>::new(), "unknown keys are plain words");
}

#[test]
fn pinned_families_sort_first_and_survive_backup() {
    let fx = fixture!();
    if !put_mixed(&fx) {
        return;
    }
    let consolas = fx.family("Consolas").key;
    fx.engine.set_pinned(std::slice::from_ref(&consolas), true).unwrap();
    assert!(fx.family("Consolas").pinned && !fx.family("Arial").pinned);
    for sort in [SortBy::Name, SortBy::NameDesc, SortBy::Date, SortBy::Foundry, SortBy::Random] {
        let q = FamilyQuery { sort, seed: Some(3.0), ..Default::default() };
        assert_eq!(fx.engine.list_families(&q).unwrap()[0].key, consolas);
    }
    let q = FamilyQuery { search: Some("pinned".into()), ..Default::default() };
    assert_eq!(fx.engine.list_families(&q).unwrap().len(), 1);
    assert!(fx.engine.get_family(&consolas).unwrap().family.pinned);

    let file = fx.data.join("pins.typeshelf");
    fx.engine.backup_create(&file).unwrap();
    fx.engine.set_pinned(std::slice::from_ref(&consolas), false).unwrap();
    assert!(!fx.family("Consolas").pinned);
    fx.engine.backup_restore(&file).unwrap();
    assert!(fx.family("Consolas").pinned);
}

#[test]
fn details_and_glyphs_from_the_file() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let arial = fx.family("Arial");
    let d = fx.engine.get_family(&arial.key).unwrap().details;
    assert!(d.copyright.as_deref().is_some_and(|c| !c.is_empty()));
    assert_eq!((d.format.as_str(), d.upm), ("TrueType", Some(2048)));
    assert!(d.glyph_count > 1000);
    let json = serde_json::to_value(fx.engine.get_family(&arial.key).unwrap()).unwrap();
    assert!(json["details"].get("licenseUrl").is_some() && json["details"].get("glyphCount").is_some());

    let map = fx.engine.face_glyphs(&arial.faces[0].hash, 0).unwrap();
    assert_eq!(map.blocks[0].name, "Basic Latin");
    assert!(map.blocks[0].chars.contains(&('A' as u32)));
    assert!(map.total > 1000);
    assert_eq!(fx.engine.face_glyphs("nope", 0).unwrap_err().code, "not_found");
}

#[test]
fn collections_export_import_round_trip() {
    let fx = fixture!();
    if !put_mixed(&fx) {
        return;
    }
    let cols = fx.engine.create_collection("Parent", None).unwrap();
    let parent = cols.iter().find(|c| c.name == "Parent").unwrap().id;
    let cols = fx.engine.create_collection("Child", Some(parent)).unwrap();
    let child = cols.iter().find(|c| c.name == "Child").unwrap().id;
    fx.engine.add_to_collection(parent, &[fx.family("Arial").key]).unwrap();
    fx.engine.add_to_collection(child, &[fx.family("Consolas").key]).unwrap();
    let file = fx.data.join("collections.json");
    // Parent, Child and the built-in Favorites (Recent is left out).
    assert_eq!(fx.engine.export_collections(&file).unwrap().collections, 3);

    fx.engine.delete_collection(parent).unwrap();
    let res = fx.engine.import_collections(&file).unwrap();
    assert_eq!((res.collections, res.families, res.missing), (3, 2, 0));
    let count = |name: &str| fx.engine.list_collections().unwrap().into_iter().filter(|c| c.name == name).collect::<Vec<_>>();
    let parent = count("Parent");
    assert_eq!((parent.len(), parent[0].count), (1, 1));
    assert_eq!(count("Child")[0].parent_id, Some(parent[0].id));
    // Importing again merges instead of duplicating.
    fx.engine.import_collections(&file).unwrap();
    assert_eq!((count("Parent").len(), count("Child").len(), count("Favorites").len()), (1, 1, 1));

    // Unknown keys fall back to the family name; no match at all counts as missing.
    let other = fx.data.join("other.json");
    std::fs::write(
        &other,
        r#"{ "version": 1, "collections": [ { "name": "Picked", "families": [
            { "key": "gone|x", "family": "times new roman" }, { "key": "gone|y", "family": "Nope" } ] } ] }"#,
    )
    .unwrap();
    let res = fx.engine.import_collections(&other).unwrap();
    assert_eq!((res.collections, res.families, res.missing), (1, 1, 1));
    let q = FamilyQuery { collection_id: Some(count("Picked")[0].id), ..Default::default() };
    assert_eq!(fx.engine.list_families(&q).unwrap()[0].family, "Times New Roman");
    std::fs::write(&other, "nope").unwrap();
    assert_eq!(fx.engine.import_collections(&other).unwrap_err().code, "invalid");
}

#[test]
fn sample_text_presets_default_for_old_settings() {
    let fx = fixture!();
    assert_eq!(fx.engine.get_settings().unwrap().sample_texts.len(), 5);
    store::set(&fx.engine.db(), "app", &serde_json::json!({ "theme": "dark", "sampleText": "old" })).unwrap();
    let s = fx.engine.get_settings().unwrap();
    assert_eq!((s.theme, s.sample_text.as_str(), s.sample_texts.len()), (Theme::Dark, "old", 5));
    let s = fx.engine.update_settings(serde_json::json!({ "sampleTexts": ["One"] })).unwrap();
    assert_eq!(s.sample_texts, vec!["One"]);
}

#[test]
fn big_batches_are_chunked_with_one_broadcast() {
    let fx = fixture!();
    // ~100 distinct fonts from the system folder, plus an older Arial that must lose to the newer one.
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let mut n = 0;
    for e in std::fs::read_dir(Path::new(&windir).join("Fonts")).unwrap().flatten() {
        let p = e.path();
        let name = p.file_name().unwrap().to_string_lossy().to_lowercase();
        if n < 100 && name.ends_with(".ttf") && !name.starts_with("arial") {
            fx.put(&format!("many/{name}"), &std::fs::read(&p).unwrap());
            n += 1;
        }
    }
    let arial = system_font("arial.ttf").unwrap();
    let (old, _) = with_version(&arial, "0.01");
    let new_path = fx.put("arial.ttf", &arial);
    fx.put("old/arial-old.ttf", &old);
    fx.engine.add_folder(&fx.root).unwrap();
    let progress = Arc::new(Mutex::new(Vec::new()));
    let sink = progress.clone();
    fx.engine.on_ui_event(move |e| {
        if let UiEvent::ActivationProgress { done, total, .. } = e {
            sink.lock().unwrap().push((done, total));
        }
    });
    let before = *fx.os.0.notifies.lock().unwrap();

    let all: Vec<String> = fx.engine.list_families(&FamilyQuery::default()).unwrap().iter().flat_map(|f| f.faces.iter().map(|x| x.hash.clone())).collect();
    assert!(all.len() > 64, "needs more than one chunk, got {}", all.len());
    let results = fx.engine.set_active(&all, true).unwrap();
    assert_eq!(results.len(), all.len());
    assert!(fx.os.registered().contains(&new_path), "newest Arial wins even though the batch was chunked");
    assert_eq!(results.iter().filter(|r| r.error == Some(ActivationErrorCode::NameConflict)).count(), 1);
    assert_eq!(*fx.os.0.notifies.lock().unwrap() - before, 1, "one font-change broadcast for the whole batch");
    let p = progress.lock().unwrap().clone();
    assert!(p.len() >= 2 && p.last().unwrap().0 == all.len(), "progress {p:?}");

    fx.engine.set_active(&all, false).unwrap();
    assert!(fx.os.registered().is_empty());
    assert_eq!(*fx.os.0.notifies.lock().unwrap() - before, 2);
}

#[test]
fn background_activation_returns_first_and_reports_failures() {
    let Some(arial) = system_font("arial.ttf") else { return };
    let Some(times) = system_font("times.ttf") else { return };
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("fonts");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("arial.ttf"), &arial).unwrap();
    std::fs::write(root.join("times.ttf"), &times).unwrap();
    let os = FakeOs::default();
    let engine = Arc::new(Engine::open(tmp.path().join("data"), os.clone()).unwrap());
    engine.activate_in_background();
    engine.add_folder(&root).unwrap();
    let done = Arc::new(Mutex::new(Vec::new()));
    let sink = done.clone();
    engine.on_ui_event(move |e| {
        if let UiEvent::ActivationDone { failed, .. } = e {
            sink.lock().unwrap().push(failed);
        }
    });
    let hashes = |name: &str| -> Vec<String> {
        let all = engine.list_families(&FamilyQuery::default()).unwrap();
        all.into_iter().find(|f| f.family == name).unwrap().faces.into_iter().map(|f| f.hash).collect()
    };
    let (a, t) = (hashes("Arial"), hashes("Times New Roman"));
    os.0.fail.lock().unwrap().insert(std::path::absolute(root.join("times.ttf")).unwrap());

    // Returns while the OS is still busy: the ledger already says "on".
    let gate = os.0.gate.lock().unwrap();
    let r = engine.set_active(&[a.clone(), t.clone()].concat(), true).unwrap();
    assert!(r.iter().all(|r| r.ok));
    assert!(os.registered().is_empty(), "nothing registered yet");
    assert_eq!(engine.library_counts().unwrap().active, 2, "but the library already shows both as on");
    drop(gate);
    engine.wait_for_activations();

    assert_eq!(os.registered().len(), 1, "Arial registered, Times failed");
    assert_eq!(engine.library_counts().unwrap().active, 1, "the failed one is switched back off");
    let failed = done.lock().unwrap().clone();
    assert_eq!(failed.len(), 1);
    assert!(failed[0].len() == 1 && failed[0][0].contains("Times"), "{failed:?}");

    // Turned off while still queued: it must not end up registered.
    engine.set_active(&a, false).unwrap();
    assert!(os.registered().is_empty());
    let gate = os.0.gate.lock().unwrap();
    engine.set_active(&a, true).unwrap();
    engine.set_active(&a, false).unwrap();
    drop(gate);
    engine.wait_for_activations();
    assert!(os.registered().is_empty(), "registration that was cancelled is undone");
    assert_eq!(engine.library_counts().unwrap().active, 0);
}

#[test]
fn a_stalled_os_call_does_not_lock_the_library() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let hashes = fx.hashes("Arial");
    fx.engine.set_active(&hashes, true).unwrap();
    let engine = Arc::new(fx.engine);
    let stall = fx.os.0.stall.lock().unwrap();

    // Repair and turning off both wait on the "OS"; the library must still answer meanwhile.
    let (e1, e2) = (engine.clone(), engine.clone());
    let repair = std::thread::spawn(move || e1.repair_now().map(|_| ()));
    let off = std::thread::spawn(move || e2.set_active(&hashes, false).map(|_| ()));
    let (tx, rx) = std::sync::mpsc::channel();
    let e3 = engine.clone();
    std::thread::spawn(move || tx.send(e3.library_counts().map(|c| c.active)).unwrap());
    let active = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("library blocked by a stalled OS call");
    assert_eq!(active.unwrap(), 1, "still on until the OS call returns");

    drop(stall);
    repair.join().unwrap().unwrap();
    off.join().unwrap().unwrap();
    assert_eq!(engine.library_counts().unwrap().active, 0);
}

#[test]
fn collections_report_how_many_families_are_on() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    let Some(times) = system_font("times.ttf") else { return };
    fx.put("times.ttf", &times);
    fx.engine.add_folder(&fx.root).unwrap();
    let id = fx.engine.create_collection("Pair", None).unwrap().into_iter().find(|c| c.name == "Pair").unwrap().id;
    let keys = vec![fx.family("Arial").key, fx.family("Times New Roman").key];
    fx.engine.add_to_collection(id, &keys).unwrap();
    let state = |fx: &Fixture| {
        let c = fx.engine.list_collections().unwrap().into_iter().find(|c| c.id == id).unwrap();
        (c.active, c.count)
    };
    assert_eq!(state(&fx), (0, 2), "none on");
    fx.engine.set_active(&fx.hashes("Arial"), true).unwrap();
    assert_eq!(state(&fx), (1, 2), "some on");
    fx.engine.set_active(&fx.hashes("Times New Roman"), true).unwrap();
    assert_eq!(state(&fx), (2, 2), "all on");
}

mod audit;
mod merged;
mod formats;
mod languages;
mod temporary;
mod stats;
mod docfonts;
