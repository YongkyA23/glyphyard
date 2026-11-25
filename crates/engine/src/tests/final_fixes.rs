//! Final review fixes (E1-E6).

use super::stats::{earned, eleven_on, usage_rows};
use super::*;

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
