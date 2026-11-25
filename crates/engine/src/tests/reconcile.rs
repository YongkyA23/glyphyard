//! Task 14: the organize journal written step by step, and the ledger reconciled after every scan.

use super::*;
use crate::organize::{journal_open, journal_step, Done};

fn arial_on(fx: &Fixture, rel: &str) -> (PathBuf, String) {
    let bytes = system_font("arial.ttf").unwrap();
    let path = fx.put(rel, &bytes);
    fx.engine.add_folder(&fx.root).unwrap();
    let hash = typeshelf_core::file_hash(&bytes);
    assert!(fx.engine.set_active(std::slice::from_ref(&hash), true).unwrap()[0].ok);
    fx.engine.wait_for_activations();
    assert_eq!(fx.os.registered(), HashSet::from([path.clone()]));
    (path, hash)
}

fn ledger_paths(fx: &Fixture) -> Vec<PathBuf> {
    let db = fx.engine.db();
    let mut stmt = db.prepare("SELECT path FROM activations").unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().map(|r| PathBuf::from(r.unwrap())).collect()
}

#[test]
fn journal_survives_a_crash_part_way() {
    let fx = fixture!();
    let bytes = system_font("arial.ttf").unwrap();
    let a = fx.put("a/arial.ttf", &bytes);
    let b = fx.put("b/arial.ttf", &bytes);
    fx.engine.add_folder(&fx.root).unwrap();

    // Step 1 is done and journalled; the "process dies" before step 2, so the entry is never finished.
    let moved = fx.root.join("sorted/arial.ttf");
    let id = {
        let db = fx.engine.db();
        let id = journal_open(&db).unwrap();
        std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
        std::fs::rename(&a, &moved).unwrap();
        journal_step(&db, id, &Done::Moved { from: a.clone(), to: moved.clone(), override_hash: None }).unwrap();
        id
    };
    let entry = fx.engine.list_history().unwrap().into_iter().find(|h| h.id == id).expect("the entry exists after the crash");
    assert_eq!((entry.kind.as_str(), entry.summary.as_str(), entry.undone), ("organize", "Auto-organize: 1 changes", false));

    fx.engine.undo_history(id).unwrap();
    assert!(a.is_file() && !moved.exists() && b.is_file(), "undo reverses the recorded step");
}

#[test]
fn a_failed_later_step_keeps_the_earlier_ones_undoable() {
    let fx = fixture!();
    let Some(times) = system_font("times.ttf") else { return };
    let arial = system_font("arial.ttf").unwrap();
    fx.put("a/arial.ttf", &arial);
    fx.put("b/arial.ttf", &arial);
    fx.put("a/times.ttf", &times);
    fx.put("b/times.ttf", &times);
    fx.engine.add_folder(&fx.root).unwrap();
    let plan = fx.engine.organize_plan(false).unwrap();
    let trashes: Vec<(PathBuf, PathBuf)> = plan
        .changes
        .iter()
        .filter_map(|c| match &c.change.action {
            typeshelf_core::organize::Action::Trash { path, kept } => Some((path.clone(), kept.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(trashes.len(), 2, "{:?}", plan.changes);

    // The second step fails: the copy it wanted to keep is gone.
    std::fs::remove_file(&trashes[1].1).unwrap();
    let result = fx.engine.organize_apply(false, &[]).unwrap();
    assert_eq!((result.applied, result.failed.len()), (1, 1));
    assert!(!trashes[0].0.exists() && trashes[1].0.exists());
    let entry = fx.engine.list_history().unwrap().into_iter().find(|h| h.id == result.history_id).unwrap();
    assert_eq!(entry.summary, "Auto-organize: 1 changes");

    fx.engine.undo_history(result.history_id).unwrap();
    assert!(trashes[0].0.is_file(), "the step that was done is undone");
}

#[test]
fn an_apply_that_changes_nothing_leaves_no_history() {
    let fx = fixture!();
    let bytes = system_font("arial.ttf").unwrap();
    fx.put("a/arial.ttf", &bytes);
    let kept = fx.put("b/arial.ttf", &bytes);
    fx.engine.add_folder(&fx.root).unwrap();
    fx.engine.organize_plan(false).unwrap();
    std::fs::remove_file(&kept).unwrap();
    std::fs::remove_file(fx.root.join("a/arial.ttf")).unwrap();
    let result = fx.engine.organize_apply(false, &[]).unwrap();
    assert_eq!((result.applied, result.history_id), (0, 0), "no id that a later entry could reuse");
    assert!(fx.engine.list_history().unwrap().is_empty());
}

#[test]
fn a_font_moved_to_a_later_added_folder_stays_active() {
    let fx = fixture!();
    let old = fx.put("one/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.put("two/keep.txt", b"x");
    fx.engine.add_folder(fx.root.join("one")).unwrap();
    fx.engine.add_folder(fx.root.join("two")).unwrap();
    fx.engine.set_active(&fx.hashes("Arial"), true).unwrap();
    let new = fx.root.join("two").join("arial.ttf");
    std::fs::rename(&old, &new).unwrap();
    fx.engine.rescan().unwrap(); // folder "one" (lower id) is scanned first
    assert_eq!(fx.os.registered(), HashSet::from([new.clone()]));
    assert_eq!(ledger_paths(&fx), vec![new]);
}

#[test]
fn following_twice_from_a_stale_decision_keeps_the_font_registered() {
    let fx = fixture!();
    let (old, hash) = arial_on(&fx, "a/arial.ttf");
    let row = crate::activation::LedgerRow {
        hash,
        path: ledger_paths(&fx)[0].to_string_lossy().into_owned(), // as stored (separators as scanned)
        full_name: fx.family("Arial").faces[0].full_name.clone(),
        source: Source::User,
    };
    let new = fx.root.join("b").join("arial.ttf");
    std::fs::create_dir_all(new.parent().unwrap()).unwrap();
    std::fs::rename(&old, &new).unwrap();
    // Two reconciliations that both decided "follow" before either applied it.
    fx.engine.follow(&row, new.clone()).unwrap();
    fx.engine.follow(&row, new.clone()).unwrap();
    assert_eq!(fx.os.registered(), HashSet::from([new.clone()]));
    assert_eq!(ledger_paths(&fx), vec![new]);
}

#[test]
fn an_active_file_moved_by_the_user_stays_active() {
    let fx = fixture!();
    let (old, _) = arial_on(&fx, "a/arial.ttf");
    let new = fx.root.join("b/renamed.ttf");
    std::fs::create_dir_all(new.parent().unwrap()).unwrap();
    std::fs::rename(&old, &new).unwrap();
    fx.engine.rescan().unwrap();
    assert_eq!(fx.os.registered(), HashSet::from([new.clone()]));
    assert_eq!(ledger_paths(&fx), vec![new.clone()]);
    assert_eq!(fx.engine.library_counts().unwrap().active, 1);
}

#[test]
fn an_active_file_deleted_on_disk_is_turned_off() {
    let fx = fixture!();
    let (path, _) = arial_on(&fx, "a/arial.ttf");
    std::fs::remove_file(&path).unwrap();
    fx.engine.rescan().unwrap();
    assert!(fx.os.registered().is_empty());
    assert!(ledger_paths(&fx).is_empty());
    let events = fx.events.lock().unwrap();
    assert!(events.iter().any(|e| matches!(e, Event::FontDeactivated { .. })));
    assert!(
        events.iter().any(|e| matches!(e, Event::ActivationFailed { font, reason } if reason == "file_missing" && font.family == "Arial")),
        "reported by name"
    );
}

#[test]
fn an_active_file_replaced_with_other_bytes_is_turned_off() {
    let fx = fixture!();
    let (path, hash) = arial_on(&fx, "a/arial.ttf");
    let (other, _) = with_version(&system_font("arial.ttf").unwrap(), "0.01");
    std::fs::write(&path, &other).unwrap();
    fx.engine.db().execute("UPDATE files SET mtime = -1", []).unwrap(); // same size, maybe same second
    fx.engine.rescan().unwrap();
    assert!(fx.os.registered().is_empty());
    assert!(ledger_paths(&fx).is_empty());
    assert!(fx.events.lock().unwrap().iter().any(|e| matches!(e, Event::ActivationFailed { font, .. } if font.hash == hash)));
}

#[test]
fn a_replaced_file_in_an_auto_folder_turns_the_new_font_on() {
    let fx = fixture!();
    let Some(times) = system_font("times.ttf") else { return };
    let path = fx.put("a/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    fx.engine.set_folder_auto(&fx.root.join("a").to_string_lossy(), true).unwrap();
    assert_eq!(fx.os.registered(), HashSet::from([path.clone()]));

    std::fs::write(&path, &times).unwrap(); // another font (another PostScript name) at the same path
    fx.engine.rescan().unwrap();
    let hashes: Vec<String> =
        fx.engine.db().prepare("SELECT hash FROM activations").unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    assert_eq!(hashes, vec![typeshelf_core::file_hash(&times)], "the old row is gone, the new font is on");
    assert_eq!(fx.os.registered(), HashSet::from([path]), "and really registered");
}

#[test]
fn a_folder_that_fails_to_scan_does_not_skip_the_others() {
    let fx = fixture!();
    let (path, _) = arial_on(&fx, "a/arial.ttf");
    let id = fx.engine.list_folders().unwrap()[0].id;
    std::fs::remove_file(&path).unwrap();
    let err = fx.engine.scan_folders(&[id, 9999]).unwrap_err(); // 9999: no such folder
    assert_eq!(err.code, "not_found", "the error is still returned");
    assert!(fx.os.registered().is_empty() && ledger_paths(&fx).is_empty(), "the folder that scanned was reconciled");
}

#[test]
fn an_offline_folder_keeps_its_fonts_on() {
    let fx = fixture!();
    let path = fx.put("lib/arial.ttf", &system_font("arial.ttf").unwrap());
    let lib = fx.root.join("lib");
    fx.engine.add_folder(&lib).unwrap();
    fx.engine.set_active(&fx.hashes("Arial"), true).unwrap();
    let away = fx.root.parent().unwrap().join("far/away/deeper");
    std::fs::create_dir_all(away.parent().unwrap()).unwrap();
    std::fs::rename(&lib, &away).unwrap();
    fx.engine.rescan().unwrap();
    assert_eq!(ledger_paths(&fx), vec![path], "offline is not gone");
}
