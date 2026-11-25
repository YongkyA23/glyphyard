//! "Select all N results": the engine turns a Library query into every matching family, so bulk actions reach
//! all of them and not just the page on screen.

use super::*;

fn put_three(fx: &Fixture) -> bool {
    let fonts = ["arial.ttf", "georgia.ttf", "verdana.ttf"].map(|n| (n, system_font(n)));
    if fonts.iter().any(|(_, b)| b.is_none()) {
        return false;
    }
    for (n, b) in fonts {
        fx.put(n, &b.unwrap());
    }
    fx.engine.add_folder(&fx.root).unwrap();
    true
}

#[test]
fn targets_are_every_match_of_the_query_in_list_order() {
    let fx = fixture!();
    if !put_three(&fx) {
        return eprintln!("georgia/verdana not found, skipping");
    }
    let q = FamilyQuery { search: Some("a".into()), sort: SortBy::NameDesc, ..Default::default() };
    let listed = fx.engine.list_families(&q).unwrap();
    assert_eq!(listed.len(), 3, "every family name has an a");
    let t = fx.engine.query_targets(&q).unwrap();
    assert_eq!(t.keys, listed.iter().map(|f| f.key.clone()).collect::<Vec<_>>());
    let all: Vec<String> = listed.iter().flat_map(|f| f.faces.iter().map(|x| x.hash.clone())).collect();
    assert_eq!(t.hashes, all);

    // A narrower query: only its families, and turning on the targets turns on exactly those.
    let q = FamilyQuery { search: Some("ver".into()), ..Default::default() };
    let t = fx.engine.query_targets(&q).unwrap();
    assert_eq!(t.keys, vec![fx.family("Verdana").key]);
    fx.engine.set_active(&t.hashes, true).unwrap();
    assert_eq!(fx.family("Verdana").active_state, ActiveState::All);
    assert_eq!(fx.family("Arial").active_state, ActiveState::None);
    assert_eq!(fx.family("Georgia").active_state, ActiveState::None);

    // Family-level bulk actions take the keys.
    let all = fx.engine.query_targets(&FamilyQuery::default()).unwrap();
    let cols = fx.engine.create_collection("Everything", None).unwrap();
    let id = cols.iter().find(|c| c.name == "Everything").unwrap().id;
    fx.engine.add_to_collection(id, &all.keys).unwrap();
    assert_eq!(fx.engine.list_families(&FamilyQuery { collection_id: Some(id), ..Default::default() }).unwrap().len(), 3);
}

#[test]
fn tags_are_added_and_removed_across_all_targets_in_one_call() {
    let fx = fixture!();
    if !put_three(&fx) {
        return eprintln!("georgia/verdana not found, skipping");
    }
    let (arial, georgia, verdana) = (fx.family("Arial").key, fx.family("Georgia").key, fx.family("Verdana").key);
    fx.engine.set_tags(&arial, &["old".into(), "web".into()]).unwrap();
    fx.engine.set_tags(&georgia, &["web".into()]).unwrap();
    fx.engine.set_tags(&verdana, &["keep".into()]).unwrap();

    let keys = fx.engine.query_targets(&FamilyQuery::default()).unwrap().keys;
    // " web " trims to "web" (the set_tags rule), blank names are dropped; "old" goes wherever it is.
    let changed = fx.engine.edit_tags(&keys, &[" web ".into(), "  ".into()], &["old".into()]).unwrap();
    assert_eq!(changed, 2, "Georgia already had web and no old: unchanged");
    let tags = |name: &str| fx.family(name).tags;
    assert_eq!(tags("Arial"), vec!["web"]);
    assert_eq!(tags("Georgia"), vec!["web"]);
    assert_eq!(tags("Verdana"), vec!["keep", "web"]);
    assert!(fx.engine.list_tags().unwrap().iter().all(|t| !t.name.trim().is_empty() && t.name != "old"));

    // A tag both added and removed is removed; nothing changes for families outside the keys.
    assert_eq!(fx.engine.edit_tags(std::slice::from_ref(&arial), &["web".into()], &["web".into()]).unwrap(), 1);
    assert!(tags("Arial").is_empty());
    assert_eq!(tags("Georgia"), vec!["web"]);
}

#[test]
fn system_fonts_and_empty_queries_give_nothing_to_switch() {
    let fx = fixture!();
    let sys = fx.root.parent().unwrap().join("os-fonts");
    std::fs::create_dir_all(&sys).unwrap();
    std::fs::write(sys.join("arial.ttf"), system_font("arial.ttf").unwrap()).unwrap();
    *fx.os.0.font_dirs.lock().unwrap() = vec![sys];
    fx.engine.rescan().unwrap();

    let t = fx.engine.query_targets(&FamilyQuery { filter: FamilyFilter::System, ..Default::default() }).unwrap();
    assert_eq!(t.keys.len(), 1, "the system family is a target for tags and collections");
    assert!(t.hashes.is_empty(), "but its fonts are always on and never switched");

    let none = fx.engine.query_targets(&FamilyQuery { search: Some("no such font".into()), ..Default::default() }).unwrap();
    assert!(none.keys.is_empty() && none.hashes.is_empty());
}
