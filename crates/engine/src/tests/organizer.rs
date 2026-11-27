//! Collections and project sets as the synced organizer document: stamps, tombstones, and applying a document.

use super::*;
use std::collections::BTreeSet;
use typeshelf_core::organizer::{merge, OrganizerDoc, SyncCollection, SyncItem, SyncSet, FAVORITES};

fn engine() -> (tempfile::TempDir, Engine) {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(tmp.path().join("data"), FakeOs::default()).unwrap();
    (tmp, e)
}

fn id_of(e: &Engine, name: &str) -> i64 {
    e.list_collections().unwrap().into_iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no collection {name}")).id
}

fn set_id(e: &Engine, name: &str) -> i64 {
    e.list_project_sets().unwrap().into_iter().find(|s| s.name == name).unwrap_or_else(|| panic!("no set {name}")).id
}

fn keys(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// (name, parent name, families) of every collection but Recent.
fn tree(e: &Engine) -> BTreeSet<(String, Option<String>, Vec<String>)> {
    let db = e.db();
    let mut stmt = db
        .prepare(
            "SELECT c.name, p.name, (SELECT group_concat(family_key, ',') FROM (SELECT family_key FROM collection_items
                 WHERE collection_id = c.id ORDER BY family_key))
             FROM collections c LEFT JOIN collections p ON p.id = c.parent_id WHERE c.builtin IS NOT 'recent'",
        )
        .unwrap();
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<String>>(2)?))).unwrap();
    rows.map(|r| r.unwrap()).map(|(n, p, f)| (n, p, f.map(|f| f.split(',').map(String::from).collect()).unwrap_or_default())).collect()
}

fn sets(e: &Engine) -> BTreeSet<(String, String, Vec<String>)> {
    e.list_project_sets().unwrap().into_iter().map(|s| (s.name, s.note, s.family_keys)).collect()
}

fn coll<'a>(doc: &'a OrganizerDoc, name: &str) -> &'a SyncCollection {
    doc.collections.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("{name} not in {doc:#?}"))
}

fn item<'a>(items: &'a [SyncItem], key: &str) -> &'a SyncItem {
    items.iter().find(|i| i.key == key).unwrap_or_else(|| panic!("{key} not in {items:?}"))
}

/// Exports (what the sync round pushes) and marks that as synced.
fn synced(e: &Engine) {
    e.organizer_doc().unwrap();
    e.set_organizer_synced(1).unwrap();
    assert!(!e.organizer_sync_state().unwrap().1);
}

fn dirty(e: &Engine) -> bool {
    e.organizer_sync_state().unwrap().1
}

#[test]
fn migration_backfills_uids() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    {
        // A database from before sync: the first seven migrations, one collection, one set.
        let conn = rusqlite::Connection::open(data.join("typeshelf.db")).unwrap();
        for m in &store::MIGRATIONS[..7] {
            conn.execute_batch(m).unwrap();
        }
        conn.execute_batch(
            "PRAGMA user_version = 7;
             INSERT INTO collections (name) VALUES ('Old');
             INSERT INTO project_sets (name) VALUES ('Set A');
             INSERT INTO project_set_items (set_id, family_key) VALUES (1, 'k|f');",
        )
        .unwrap();
    }
    let e = Engine::open(&data, FakeOs::default()).unwrap();
    let uid = |sql: &str| e.db().query_row(sql, [], |r| r.get::<_, Option<String>>(0)).unwrap();
    assert_eq!(uid("SELECT uid FROM collections WHERE builtin = 'favorites'").as_deref(), Some(FAVORITES));
    assert_eq!(uid("SELECT uid FROM collections WHERE builtin = 'recent'"), None, "Recent never syncs");
    let hex = |u: Option<String>| u.is_some_and(|u| u.len() == 32 && u.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert!(hex(uid("SELECT uid FROM collections WHERE name = 'Old'")));
    assert!(hex(uid("SELECT uid FROM project_sets WHERE name = 'Set A'")));
    assert!(uid("SELECT updated_at FROM collections WHERE name = 'Old'").is_some());
    assert!(uid("SELECT added_at FROM project_set_items").is_some());
    assert_eq!(e.organizer_sync_state().unwrap(), (0, false));

    let doc = e.organizer_doc().unwrap();
    assert!(doc.collections.iter().any(|c| c.uid == FAVORITES));
    assert!(doc.collections.iter().all(|c| c.name != "Recent"));
    assert_eq!(coll(&doc, "Old").parent, None);
    assert_eq!(doc.sets.len(), 1);
    assert_eq!(doc.sets[0].items[0].key, "k|f");
    assert!(doc.sets[0].items[0].at.ends_with('Z') && doc.sets[0].items[0].at.len() == 27, "canonical: {}", doc.sets[0].items[0].at);
}

#[test]
fn mutations_mark_dirty_and_stamp() {
    let (_t, e) = engine();
    assert_eq!(e.organizer_sync_state().unwrap(), (0, false));

    e.create_collection("Client X", None).unwrap();
    assert!(dirty(&e), "create");
    let x = coll(&e.organizer_doc().unwrap(), "Client X").clone();
    assert_eq!(x.uid.len(), 32);
    synced(&e);
    assert_eq!(e.organizer_sync_state().unwrap().0, 1);

    let id = id_of(&e, "Client X");
    e.rename_collection(id, "Client Y").unwrap();
    assert!(dirty(&e), "rename");
    let y = coll(&e.organizer_doc().unwrap(), "Client Y").clone();
    assert_eq!(y.uid, x.uid, "same collection");
    assert!(y.updated_at > x.updated_at);
    synced(&e);

    e.add_to_collection(id, &keys(&["a|x"])).unwrap();
    assert!(dirty(&e), "add");
    let added = item(&coll(&e.organizer_doc().unwrap(), "Client Y").items, "a|x").clone();
    assert!(!added.removed);
    synced(&e);
    e.add_to_collection(id, &keys(&["a|x"])).unwrap();
    assert!(!dirty(&e), "adding a family that is already there changes nothing");

    e.remove_from_collection(id, &keys(&["a|x"])).unwrap();
    assert!(dirty(&e), "remove");
    let doc = e.organizer_doc().unwrap();
    let removed = item(&coll(&doc, "Client Y").items, "a|x").clone();
    assert!(removed.removed && removed.at > added.at);
    assert_eq!(coll(&doc, "Client Y").updated_at, y.updated_at, "families don't touch the own fields");
    e.add_to_collection(id, &keys(&["a|x"])).unwrap();
    let again = item(&coll(&e.organizer_doc().unwrap(), "Client Y").items, "a|x").clone();
    assert!(!again.removed && again.at > removed.at, "a re-add beats the removal");

    // Delete: the collection and its child become tombstones that keep their real updated_at.
    e.create_collection("Logos", Some(id)).unwrap();
    let before = e.organizer_doc().unwrap();
    synced(&e);
    e.delete_collection(id).unwrap();
    assert!(dirty(&e), "delete");
    let doc = e.organizer_doc().unwrap();
    for name in ["Client Y", "Logos"] {
        let (was, now) = (coll(&before, name), coll(&doc, name));
        assert_eq!(now.updated_at, was.updated_at, "{name}");
        assert!(now.deleted_at.as_ref().is_some_and(|d| *d > now.updated_at), "{name}: {now:?}");
    }
    assert_eq!(coll(&doc, "Client Y").items, coll(&before, "Client Y").items, "a user delete keeps its families");

    // Project sets: create, item diff, own fields only bump when they change, delete.
    synced(&e);
    e.save_project_set(None, "Set", "note", &keys(&["a|x", "b|y"])).unwrap();
    assert!(dirty(&e), "save new set");
    let s1 = e.organizer_doc().unwrap().sets[0].clone();
    assert_eq!(s1.items.iter().map(|i| (i.key.as_str(), i.removed)).collect::<Vec<_>>(), [("a|x", false), ("b|y", false)]);
    synced(&e);
    let sid = set_id(&e, "Set");
    e.save_project_set(Some(sid), "Set", "note", &keys(&["b|y"])).unwrap();
    assert!(dirty(&e), "save set items");
    let s2 = e.organizer_doc().unwrap().sets[0].clone();
    assert_eq!(s2.updated_at, s1.updated_at, "name and note unchanged");
    assert!(item(&s2.items, "a|x").removed);
    assert_eq!(item(&s2.items, "b|y"), item(&s1.items, "b|y"), "a kept family keeps its stamp");
    e.save_project_set(Some(sid), "Set", "new note", &keys(&["b|y"])).unwrap();
    assert!(e.organizer_doc().unwrap().sets[0].updated_at > s1.updated_at);
    synced(&e);
    e.delete_project_set(sid).unwrap();
    assert!(dirty(&e), "delete set");
    let s3 = e.organizer_doc().unwrap().sets[0].clone();
    assert!(s3.deleted_at.as_ref().is_some_and(|d| *d > s3.updated_at));
}

#[test]
fn recent_set_toggle_and_family_rename() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let e = &fx.engine;
    let key = fx.family("Arial").key;
    e.save_project_set(None, "Set", "", std::slice::from_ref(&key)).unwrap();
    e.add_to_collection(id_of(e, "Favorites"), std::slice::from_ref(&key)).unwrap();
    synced(e);

    // Turning fonts on fills Recent; turning a set on and off records is_on/last_used: neither syncs.
    e.set_active(&fx.hashes("Arial"), true).unwrap();
    let recent: i64 = e.db().query_row("SELECT COUNT(*) FROM collection_items ci JOIN collections c ON c.id = ci.collection_id WHERE c.builtin = 'recent'", [], |r| r.get(0)).unwrap();
    assert!(recent > 0);
    assert!(!dirty(e), "Recent");
    let sid = set_id(e, "Set");
    e.toggle_project_set(sid, true).unwrap();
    e.toggle_project_set(sid, false).unwrap();
    assert!(!dirty(e), "set on/off");

    // A family-key rename is a remove plus an add in Favorites and in the set.
    e.set_family_name(&fx.hashes("Arial"), Some("Renamed")).unwrap();
    assert!(dirty(e), "family-key rename");
    let new_key = fx.family("Renamed").key;
    let doc = e.organizer_doc().unwrap();
    let fav = &coll(&doc, "Favorites").items;
    assert!(item(fav, &key).removed && !item(fav, &new_key).removed);
    assert!(item(fav, &new_key).at >= item(fav, &key).at);
    let set = &doc.sets[0].items;
    assert!(item(set, &key).removed && !item(set, &new_key).removed);
}

#[test]
fn restore_and_import_stamp_what_they_touch() {
    let (t, e) = engine();
    e.create_collection("Keep", None).unwrap();
    e.add_to_collection(id_of(&e, "Keep"), &keys(&["a|x"])).unwrap();
    e.save_project_set(None, "Set", "", &keys(&["a|x"])).unwrap();
    let backup = t.path().join("b.typeshelf");
    e.backup_create(&backup).unwrap();
    let keep = coll(&e.organizer_doc().unwrap(), "Keep").clone();

    e.create_collection("Later", None).unwrap();
    e.add_to_collection(id_of(&e, "Keep"), &keys(&["b|y"])).unwrap();
    synced(&e);
    e.backup_restore(&backup).unwrap();
    assert!(dirty(&e), "restore");
    let doc = e.organizer_doc().unwrap();
    let k = coll(&doc, "Keep");
    assert_eq!(k.uid, keep.uid, "a restored collection keeps its identity");
    assert!(item(&k.items, "b|y").removed, "a family the backup lacks is removed");
    assert!(!item(&k.items, "a|x").removed);
    assert!(coll(&doc, "Later").deleted_at.is_some(), "a collection the backup lacks is deleted");
    assert!(doc.sets.iter().any(|s| s.name == "Set" && s.deleted_at.is_none()));

    synced(&e);
    let file = t.path().join("c.json");
    std::fs::write(&file, r#"{"version":1,"collections":[{"name":"Imported","families":[],"children":[]}]}"#).unwrap();
    e.import_collections(&file).unwrap();
    assert!(dirty(&e), "import");
    assert!(coll(&e.organizer_doc().unwrap(), "Imported").deleted_at.is_none());
}

#[test]
fn doc_round_trips_through_apply() {
    let (_t1, e1) = engine();
    let (_t2, e2) = engine();
    e1.create_collection("Client X", None).unwrap();
    let x = id_of(&e1, "Client X");
    e1.add_to_collection(x, &keys(&["a|1", "b|2"])).unwrap();
    e1.create_collection("Logos", Some(x)).unwrap();
    e1.add_to_collection(id_of(&e1, "Logos"), &keys(&["c|3"])).unwrap();
    e1.add_to_collection(id_of(&e1, "Favorites"), &keys(&["d|4"])).unwrap();
    e1.save_project_set(None, "Launch", "for the launch", &keys(&["a|1"])).unwrap();

    assert!(e2.apply_organizer(&e1.organizer_doc().unwrap()).unwrap());
    assert_eq!(tree(&e2), tree(&e1));
    assert_eq!(sets(&e2), sets(&e1));
    assert!(!e2.apply_organizer(&e1.organizer_doc().unwrap()).unwrap(), "applying the same doc again changes nothing");
    assert!(!dirty(&e2), "applying is not a local change");

    // Concurrent adds to the same collection on both computers both survive.
    e1.add_to_collection(x, &keys(&["p|5"])).unwrap();
    e2.add_to_collection(id_of(&e2, "Client X"), &keys(&["q|6"])).unwrap();
    let both = merge(&e1.organizer_doc().unwrap(), &e2.organizer_doc().unwrap());
    e1.apply_organizer(&both).unwrap();
    e2.apply_organizer(&both).unwrap();
    assert_eq!(tree(&e1), tree(&e2));
    let fams = tree(&e1).into_iter().find(|t| t.0 == "Client X").unwrap().2;
    assert_eq!(fams, keys(&["a|1", "b|2", "p|5", "q|6"]));
    assert_eq!(id_of(&e1, "Client X"), x, "local ids are kept");
}

#[test]
fn apply_keeps_recent_and_set_state() {
    let (_t1, e1) = engine();
    let (_t2, e2) = engine();
    e2.save_project_set(None, "Launch", "", &keys(&["a|1"])).unwrap();
    let sid = set_id(&e2, "Launch");
    e2.toggle_project_set(sid, true).unwrap();
    e2.db()
        .execute("INSERT INTO collection_items (collection_id, family_key, added_at) SELECT id, 'r|1', '2026-01-01T00:00:00Z' FROM collections WHERE builtin = 'recent'", [])
        .unwrap();
    let recent = |e: &Engine| -> Vec<(String, String)> {
        let db = e.db();
        let mut stmt = db.prepare("SELECT family_key, added_at FROM collection_items WHERE collection_id = (SELECT id FROM collections WHERE builtin = 'recent')").unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.map(|r| r.unwrap()).collect()
    };
    let (recent_before, set_before) = (recent(&e2), e2.list_project_sets().unwrap()[0].clone());

    e1.apply_organizer(&e2.organizer_doc().unwrap()).unwrap();
    let s1 = set_id(&e1, "Launch");
    assert!(!e1.list_project_sets().unwrap()[0].on, "on/off is per computer");
    e1.save_project_set(Some(s1), "Launch 2", "", &keys(&["a|1", "b|2"])).unwrap();
    assert!(e2.apply_organizer(&e1.organizer_doc().unwrap()).unwrap());

    let after = e2.list_project_sets().unwrap()[0].clone();
    assert_eq!((after.id, after.name.as_str(), after.family_keys.clone()), (sid, "Launch 2", keys(&["a|1", "b|2"])));
    assert_eq!((after.on, after.last_used), (set_before.on, set_before.last_used), "is_on and last_used untouched");
    assert_eq!(recent(&e2), recent_before, "Recent untouched");
}

#[test]
fn delete_becomes_tombstone_and_applies_remotely() {
    let (_t1, e1) = engine();
    let (_t2, e2) = engine();
    e1.create_collection("Client X", None).unwrap();
    e1.create_collection("Logos", Some(id_of(&e1, "Client X"))).unwrap();
    e1.save_project_set(None, "Launch", "", &[]).unwrap();
    let stale = e1.organizer_doc().unwrap();
    e2.apply_organizer(&stale).unwrap();
    assert_eq!(tree(&e2).len(), 3);

    e1.delete_collection(id_of(&e1, "Client X")).unwrap();
    e1.delete_project_set(set_id(&e1, "Launch")).unwrap();
    let doc = e1.organizer_doc().unwrap();
    assert!(e2.apply_organizer(&doc).unwrap());
    assert_eq!(tree(&e2).into_iter().map(|t| t.0).collect::<Vec<_>>(), ["Favorites"]);
    assert!(e2.list_project_sets().unwrap().is_empty());
    // The tombstones are kept exactly as received, and a stale copy can't bring the collections back.
    let d2 = e2.organizer_doc().unwrap();
    for name in ["Client X", "Logos"] {
        assert_eq!(coll(&d2, name), coll(&doc, name));
    }
    assert_eq!(d2.sets, doc.sets);
    assert!(!e2.apply_organizer(&stale).unwrap());
    assert_eq!(tree(&e2).len(), 1);
}

fn sc(uid: &str, name: &str, parent: Option<&str>, at: &str) -> SyncCollection {
    SyncCollection { uid: uid.into(), name: name.into(), parent: parent.map(Into::into), updated_at: at.into(), deleted_at: None, items: vec![] }
}

#[test]
fn apply_repairs_parents_and_keeps_newer_local_changes() {
    let (_t, e) = engine();
    let (t1, t2) = ("2026-01-01T00:00:01.000000Z", "2026-01-01T00:00:02.000000Z");
    let mut fav = sc(FAVORITES, "Renamed", Some("a"), t2);
    fav.items.push(SyncItem { key: "f|1".into(), at: t1.into(), removed: false });
    let doc = OrganizerDoc {
        collections: vec![sc("a", "A", Some("b"), t1), sc("b", "B", Some("a"), t2), sc("c", "C", Some("gone"), t1), fav],
        sets: vec![SyncSet { uid: "s".into(), name: "S".into(), note: String::new(), updated_at: t1.into(), deleted_at: None, items: vec![] }],
    };
    assert!(e.apply_organizer(&doc).unwrap());
    let t = tree(&e);
    assert!(t.contains(&("B".into(), None, vec![])), "the loop is cut at its newest member: {t:?}");
    assert!(t.contains(&("A".into(), Some("B".into()), vec![])));
    assert!(t.contains(&("C".into(), None, vec![])), "a missing parent means top level");
    assert!(t.contains(&("Favorites".into(), None, keys(&["f|1"]))), "Favorites keeps its name and place");

    // A change made while the round was on the network survives the apply and stays unsynced.
    let pushed = e.organizer_doc().unwrap();
    e.create_collection("New", None).unwrap();
    e.apply_organizer(&pushed).unwrap();
    assert!(tree(&e).iter().any(|t| t.0 == "New"));
    e.set_organizer_synced(2).unwrap();
    assert_eq!(e.organizer_sync_state().unwrap(), (2, true), "the new collection still needs a push");
}

#[test]
fn stamps_beat_a_clock_that_ran_ahead() {
    // Another computer's clock is an hour ahead: local edits must still win, and a delete must stay a delete.
    let (_t, e) = engine();
    let ahead = typeshelf_core::organizer::timestamp(chrono::Utc::now() + chrono::Duration::hours(1));
    let mut x = sc("x", "X", None, &ahead);
    x.items.push(SyncItem { key: "a|1".into(), at: ahead.clone(), removed: false });
    e.apply_organizer(&OrganizerDoc { collections: vec![x], sets: vec![] }).unwrap();
    let id = id_of(&e, "X");

    e.rename_collection(id, "Y").unwrap();
    e.remove_from_collection(id, &keys(&["a|1"])).unwrap();
    let y = coll(&e.organizer_doc().unwrap(), "Y").clone();
    assert!(y.updated_at > ahead, "{} vs {ahead}", y.updated_at);
    assert!(item(&y.items, "a|1").removed && item(&y.items, "a|1").at > ahead);

    e.delete_collection(id).unwrap();
    let dead = coll(&e.organizer_doc().unwrap(), "Y").clone();
    assert_eq!(dead.updated_at, y.updated_at);
    assert!(dead.deleted_at.as_ref().is_some_and(|d| *d > dead.updated_at), "{dead:?}");
}
