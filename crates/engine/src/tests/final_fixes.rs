//! Final review fixes (E1-E6).

use super::stats::{earned, eleven_on, usage_rows};
use super::*;
use chrono::{Duration, Utc};
use std::collections::BTreeMap;

fn off_rows(fx: &Fixture) -> usize {
    usage_rows(fx).iter().filter(|(active, _)| !active).count()
}

// E1: turn-offs the user did not ask for are not use, and cannot award "Quiet".

#[test]
fn deactivate_all_on_quit_is_not_use() {
    let fx = fixture!();
    let Some(_) = eleven_on(&fx) else { return };
    fx.engine.deactivate_all().unwrap();
    assert!(fx.os.registered().is_empty());
    assert_eq!(off_rows(&fx), 0, "quitting is not the user turning fonts off");
    assert_eq!(earned(&fx, "quiet"), None);
}

#[test]
fn removing_a_folder_with_active_fonts_is_not_use() {
    let fx = fixture!();
    let Some(_) = eleven_on(&fx) else { return };
    let id = fx.engine.list_folders().unwrap()[0].id;
    fx.engine.remove_folder(id).unwrap();
    assert!(fx.os.registered().is_empty());
    assert_eq!(off_rows(&fx), 0);
    assert_eq!(earned(&fx, "quiet"), None);
}

#[test]
fn an_adobe_release_is_not_use() {
    let fx = fixture!();
    let Some(hashes) = eleven_on(&fx) else { return };
    assert!(fx.engine.deactivate_unrecorded(&hashes).unwrap().iter().all(|r| r.ok));
    assert!(fx.os.registered().is_empty());
    assert_eq!(off_rows(&fx), 0);
    assert_eq!(earned(&fx, "quiet"), None);
}

#[test]
fn a_user_turning_eleven_fonts_off_still_earns_quiet() {
    let fx = fixture!();
    let Some(hashes) = eleven_on(&fx) else { return };
    fx.engine.set_active(&hashes, false).unwrap();
    assert_eq!(off_rows(&fx), 11);
    assert!(earned(&fx, "quiet").is_some());
}

#[test]
fn auto_activation_is_not_use() {
    let fx = fixture!();
    let Some(times) = system_font("times.ttf") else { return };
    fx.put("a/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    fx.engine.set_folder_auto(&fx.root.join("a").to_string_lossy(), true).unwrap();
    let before = usage_rows(&fx).len();
    let path = fx.put("a/times.ttf", &times);
    fx.engine.rescan().unwrap();
    assert!(fx.os.registered().contains(&path), "the new file was turned on");
    assert_eq!(usage_rows(&fx).len(), before, "a file landing in an auto folder is not a day of use");
}

// E2: fonts scanned before ascender/descender existed are read again.

#[test]
fn migration_7_forces_a_reread() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("t.db");
    {
        let db = crate::store::open(&path).unwrap();
        db.execute("INSERT INTO folders (path, added_at) VALUES ('C:/f', 'x')", []).unwrap();
        db.execute(
            "INSERT INTO files (folder_id, rel_path, hash, size, mtime, format, added_at) VALUES (1, 'a.ttf', 'h', 1, 123, 'ttf', 'x')",
            [],
        )
        .unwrap();
        db.execute_batch("PRAGMA user_version = 6").unwrap();
    }
    let db = crate::store::open(&path).unwrap();
    let mtime: i64 = db.query_row("SELECT mtime FROM files", [], |r| r.get(0)).unwrap();
    assert_eq!(mtime, -1);
}

// E3: temporary records.

fn temporary_records(fx: &Fixture) -> BTreeMap<String, String> {
    crate::store::get(&fx.engine.db(), "temporary").unwrap().unwrap_or_default()
}

#[test]
fn a_system_font_turned_on_for_a_while_gets_no_record() {
    let fx = fixture!();
    let sys = fx.root.parent().unwrap().join("os-fonts");
    std::fs::create_dir_all(&sys).unwrap();
    std::fs::write(sys.join("arial.ttf"), system_font("arial.ttf").unwrap()).unwrap();
    *fx.os.0.font_dirs.lock().unwrap() = vec![sys];
    fx.engine.rescan().unwrap();
    let q = FamilyQuery { filter: FamilyFilter::System, ..Default::default() };
    let hashes: Vec<String> = fx.engine.list_families(&q).unwrap().into_iter().flat_map(|f| f.faces).map(|f| f.hash).collect();
    assert!(fx.engine.set_active_for(&hashes, Until::Minutes(60)).unwrap().iter().all(|r| r.ok));
    assert!(temporary_records(&fx).is_empty(), "always on, never temporary");
}

#[test]
fn an_expiry_the_os_refuses_keeps_its_record() {
    let fx = fixture!();
    let path = fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let hashes = fx.hashes("Arial");
    fx.engine.set_active_for(&hashes, Until::Minutes(5)).unwrap();
    fx.os.0.fail_off.lock().unwrap().insert(std::path::absolute(&path).unwrap());
    assert_eq!(fx.engine.expire_temporary(Utc::now() + Duration::minutes(6), false).unwrap(), 0);
    assert!(temporary_records(&fx).contains_key(&hashes[0]), "kept, so the next sweep tries again");

    fx.os.0.fail_off.lock().unwrap().clear();
    assert_eq!(fx.engine.expire_temporary(Utc::now() + Duration::minutes(6), false).unwrap(), 1);
    assert!(fx.os.registered().is_empty());
}

// E4: reconcile.

#[test]
fn a_follow_that_cannot_register_turns_the_font_off() {
    let fx = fixture!();
    let old = fx.put("a/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let hash = fx.hashes("Arial")[0].clone();
    fx.engine.set_active(std::slice::from_ref(&hash), true).unwrap();
    let new = fx.root.join("b").join("arial.ttf");
    std::fs::create_dir_all(new.parent().unwrap()).unwrap();
    std::fs::rename(&old, &new).unwrap();
    fx.os.0.fail.lock().unwrap().insert(std::path::absolute(&new).unwrap());
    fx.engine.rescan().unwrap();
    assert!(fx.os.registered().is_empty());
    let rows: i64 = fx.engine.db().query_row("SELECT COUNT(*) FROM activations", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 0, "not shown as on while unregistered");
    assert!(fx.events.lock().unwrap().iter().any(|e| matches!(e, Event::ActivationFailed { font, .. } if font.hash == hash)));
}

#[test]
fn a_partly_failed_scan_still_tells_the_ui() {
    let fx = fixture!();
    let path = fx.put("a/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let id = fx.engine.list_folders().unwrap()[0].id;
    std::fs::remove_file(&path).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    fx.engine.on_ui_event(move |e| sink.lock().unwrap().push(e));
    assert!(fx.engine.scan_folders(&[id, 9999], "scan").is_err());
    assert!(events.lock().unwrap().iter().any(|e| matches!(e, UiEvent::LibraryChanged { .. })));
}
