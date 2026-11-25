//! Regression tests for data-loss and wrong-state bugs found in the audit.

use super::*;

fn trash_of(plan: &OrganizePlan) -> (PathBuf, PathBuf) {
    plan.changes
        .iter()
        .find_map(|c| match &c.change.action {
            typeshelf_core::organize::Action::Trash { path, kept } => Some((path.clone(), kept.clone())),
            _ => None,
        })
        .expect("a trash change")
}

fn two_copies(fx: &Fixture) -> (PathBuf, PathBuf) {
    let bytes = system_font("arial.ttf").unwrap();
    fx.put("a/arial.ttf", &bytes);
    fx.put("b/arial.ttf", &bytes);
    fx.engine.add_folder(&fx.root).unwrap();
    trash_of(&fx.engine.organize_plan(false).unwrap())
}

#[test]
fn organize_never_trashes_the_last_copy() {
    let fx = fixture!();
    let (path, kept) = two_copies(&fx);

    // The copy the plan wanted to keep disappears between the preview and Apply.
    std::fs::remove_file(&kept).unwrap();
    let result = fx.engine.organize_apply(false, &[]).unwrap();
    assert!(path.exists(), "the only remaining copy must stay");
    assert_eq!(result.applied, 0);
    assert_eq!(result.failed.len(), 1, "{:?}", result.failed);
}

#[test]
fn organize_never_trashes_a_changed_keeper() {
    let fx = fixture!();
    let (path, kept) = two_copies(&fx);

    std::fs::write(&kept, b"no longer the same font").unwrap();
    let result = fx.engine.organize_apply(false, &[]).unwrap();
    assert!(path.exists(), "the kept file is not a copy any more");
    assert_eq!(result.applied, 0);
}

#[test]
fn undo_that_cannot_restore_says_so_and_stays_undoable() {
    let fx = fixture!();
    let (path, kept) = two_copies(&fx);
    let result = fx.engine.organize_apply(false, &[]).unwrap();
    assert_eq!(result.applied, 1);
    assert!(!path.exists());

    // The kept copy is gone too, so the trashed one cannot be copied back.
    let stash = fx.root.parent().unwrap().join("stash.bin");
    std::fs::rename(&kept, &stash).unwrap();
    let err = fx.engine.undo_history(result.history_id).unwrap_err();
    assert_eq!(err.code, "undo_incomplete");
    assert!(!fx.engine.list_history().unwrap()[0].undone, "a failed undo can be tried again");

    std::fs::rename(&stash, &kept).unwrap();
    assert!(fx.engine.undo_history(result.history_id).unwrap()[0].undone);
    assert!(path.exists() && kept.exists());
}

#[test]
fn locate_folder_rejects_a_path_inside_another_folder() {
    let fx = fixture!();
    fx.put("one/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.put("two/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(fx.root.join("one")).unwrap();
    let two = fx.engine.add_folder(fx.root.join("two")).unwrap().into_iter().find(|f| f.path.ends_with("two")).unwrap().id;
    std::fs::create_dir_all(fx.root.join("one").join("sub")).unwrap();
    let err = fx.engine.locate_folder(two, fx.root.join("one").join("sub")).unwrap_err();
    assert_eq!(err.code, "overlap");
}

#[test]
fn a_folder_moved_inside_another_is_not_indexed_twice() {
    let fx = fixture!();
    fx.put("one/keep.txt", b"x");
    fx.put("two/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(fx.root.join("one")).unwrap();
    fx.engine.add_folder(fx.root.join("two")).unwrap();
    std::fs::rename(fx.root.join("two"), fx.root.join("one").join("two")).unwrap();
    fx.engine.rescan().unwrap();

    let folders = fx.engine.list_folders().unwrap();
    let two = folders.iter().find(|f| f.path.ends_with("two")).unwrap();
    assert_eq!(two.status, FolderStatus::Offline, "it is not followed into the other folder");
    let files: i64 = fx
        .engine
        .db()
        .query_row("SELECT COUNT(*) FROM files f JOIN folders fo ON fo.id = f.folder_id WHERE fo.status != 'offline'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(files, 1, "one file on disk, one row");
    assert!(fx.engine.organize_plan(false).unwrap().changes.is_empty(), "nothing is its own duplicate");
}

/// Makes a folder unlistable (or listable again) for the current user.
fn set_listable(dir: &Path, on: bool) {
    #[cfg(windows)]
    {
        let args: [&str; 2] = if on { ["/remove:d", "*S-1-1-0"] } else { ["/deny", "*S-1-1-0:(RD)"] };
        let out = std::process::Command::new("icacls").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(if on { 0o755 } else { 0o000 })).unwrap();
    }
}

#[test]
fn an_unreadable_subfolder_keeps_its_fonts() {
    let fx = fixture!();
    let Some(bold) = system_font("arialbd.ttf") else { return };
    fx.put("a/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.put("b/arialbd.ttf", &bold);
    fx.engine.add_folder(&fx.root).unwrap();
    assert_eq!(fx.family("Arial").faces.len(), 2);

    let b = fx.root.join("b");
    set_listable(&b, false);
    let scanned = fx.engine.rescan();
    set_listable(&b, true);
    scanned.unwrap();
    assert_eq!(fx.family("Arial").faces.len(), 2, "a read error is not a deletion");

    // A real deletion is still picked up once the folder reads fine.
    std::fs::remove_file(b.join("arialbd.ttf")).unwrap();
    fx.engine.rescan().unwrap();
    assert_eq!(fx.family("Arial").faces.len(), 1);
}

#[test]
fn a_rename_inside_an_auto_folder_is_not_an_arrival() {
    let fx = fixture!();
    fx.put("a/arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let a = fx.root.join("a").to_string_lossy().into_owned();
    fx.engine.set_folder_auto(&a, true).unwrap();
    fx.engine.set_folder_active(&a, false).unwrap();
    std::fs::rename(fx.root.join("a").join("arial.ttf"), fx.root.join("a").join("renamed.ttf")).unwrap();
    fx.engine.rescan().unwrap();
    assert!(fx.os.registered().is_empty(), "the user turned it off; a rename must not turn it back on");
}

#[test]
fn license_edits_keep_the_other_fields() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let key = fx.family("Arial").key;
    fx.engine.set_license(&key, License::Licensed, Some("C:\\licenses\\arial.pdf"), Some("3 seats")).unwrap();

    let info = fx.engine.set_license(&key, License::Client, None, None).unwrap();
    assert_eq!((info.label, info.file.as_deref(), info.note.as_str()), (License::Client, Some("C:\\licenses\\arial.pdf"), "3 seats"));
    let info = fx.engine.set_license(&key, License::Client, None, Some("5 seats")).unwrap();
    assert_eq!((info.file.as_deref(), info.note.as_str()), (Some("C:\\licenses\\arial.pdf"), "5 seats"));
    let info = fx.engine.set_license(&key, License::Client, Some("D:\\new.pdf"), None).unwrap();
    assert_eq!((info.file.as_deref(), info.note.as_str()), (Some("D:\\new.pdf"), "5 seats"));
}

#[test]
fn blocking_a_second_file_keeps_the_first_blocked() {
    let fx = fixture!();
    let bytes = system_font("arial.ttf").unwrap();
    let (old, _) = with_version(&bytes, "0.01");
    fx.put("new/arial.ttf", &bytes);
    fx.put("old/arial.ttf", &old);
    fx.engine.add_folder(&fx.root).unwrap();
    let (h_new, h_old) = (typeshelf_core::file_hash(&bytes), typeshelf_core::file_hash(&old));
    let ps = arial_ref(&fx).ps_name;

    fx.engine.apply_command(&CommandKind::Block { ps_name: ps.clone(), hash: Some(h_new.clone()) }).unwrap();
    fx.engine.apply_command(&CommandKind::Block { ps_name: ps.clone(), hash: Some(h_old.clone()) }).unwrap();
    for h in [h_new, h_old] {
        let r = fx.engine.set_active(std::slice::from_ref(&h), true).unwrap();
        assert_eq!(r[0].error, Some(ActivationErrorCode::Blocked), "{h}");
    }
}

#[test]
fn deactivating_a_font_that_is_off_reports_no_admin_change() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let ui = Arc::new(Mutex::new(Vec::new()));
    let sink = ui.clone();
    fx.engine.on_ui_event(move |e| sink.lock().unwrap().push(e));

    fx.engine.apply_command(&CommandKind::Deactivate { font: arial_ref(&fx) }).unwrap();
    assert!(!ui.lock().unwrap().iter().any(|e| matches!(e, UiEvent::AdminChange { .. })), "no \"turned off 0 fonts\"");
}

#[test]
fn active_fonts_come_back_when_their_folder_returns() {
    let fx = fixture!();
    fx.put("lib/arial.ttf", &system_font("arial.ttf").unwrap());
    let lib = fx.root.join("lib");
    fx.engine.add_folder(&lib).unwrap();
    fx.engine.set_active(&fx.hashes("Arial"), true).unwrap();
    fx.engine.wait_for_activations();
    assert_eq!(fx.os.registered().len(), 1);

    // The drive is unplugged: startup repair drops the dead registration.
    let away = fx.root.parent().unwrap().join("unplugged").join("deep").join("x");
    std::fs::create_dir_all(away.parent().unwrap()).unwrap();
    std::fs::rename(&lib, &away).unwrap();
    fx.engine.rescan().unwrap();
    fx.engine.repair_now().unwrap();
    assert!(fx.os.registered().is_empty());

    // Plugged back in while the app runs.
    std::fs::rename(&away, &lib).unwrap();
    assert!(fx.engine.reconnect_folders().unwrap(), "a folder came back");
    assert_eq!(fx.engine.list_folders().unwrap()[0].status, FolderStatus::Watching);
    assert_eq!(fx.os.registered().len(), 1, "its active fonts are registered again");
    assert!(!fx.engine.reconnect_folders().unwrap(), "nothing to do the second time");
}

#[test]
fn folder_paths_include_the_system_folders() {
    let fx = fixture!();
    let sys = fx.root.parent().unwrap().join("os-fonts");
    std::fs::create_dir_all(&sys).unwrap();
    *fx.os.0.font_dirs.lock().unwrap() = vec![sys.clone()];
    fx.engine.add_folder(&fx.root).unwrap();
    fx.engine.rescan().unwrap();

    let paths = fx.engine.folder_paths().unwrap();
    assert!(paths.contains(&fx.root) && paths.contains(&sys), "{paths:?}");
    assert_eq!(paths.len(), 2);
}
