//! TrueType collections, variable fonts and web fonts, against whatever system fonts the machine has.

use super::*;

/// The first system font in `names` that exists; says which one on stderr.
fn first_system_font(test: &str, names: &[&str]) -> Option<(String, Vec<u8>)> {
    for n in names {
        if let Some(b) = system_font(n) {
            eprintln!("{test}: using {n}");
            return Some((n.to_string(), b));
        }
    }
    eprintln!("{test}: none of {names:?} found, skipping");
    None
}

fn all_faces(fx: &Fixture) -> Vec<Face> {
    fx.engine.list_families(&FamilyQuery::default()).unwrap().into_iter().flat_map(|f| f.faces).collect()
}

#[test]
fn collection_lists_every_face_and_turns_on_once() {
    let fx = fixture!();
    let Some((name, bytes)) = first_system_font("collection", &["cambria.ttc", "msgothic.ttc", "Nirmala.ttc", "YuGothR.ttc", "simsun.ttc", "msyh.ttc"]) else {
        return;
    };
    let expected = typeshelf_core::parse_faces(&bytes).unwrap();
    assert!(expected.len() > 1, "{name} should hold several faces");
    let path = fx.put(&name, &bytes);
    fx.engine.add_folder(&fx.root).unwrap();

    let faces: Vec<Face> = all_faces(&fx).into_iter().filter(|f| Path::new(&f.path) == path).collect();
    let mut got: Vec<&str> = faces.iter().map(|f| f.ps_name.as_str()).collect();
    got.sort();
    let mut want: Vec<&str> = expected.iter().map(|f| f.ps_name.as_str()).collect();
    want.sort();
    assert_eq!(got, want, "every face of the collection is listed, each under its own PostScript name");
    let unique: HashSet<&str> = got.iter().copied().collect();
    assert_eq!(unique.len(), got.len());

    let hashes: Vec<String> = faces.iter().map(|f| f.hash.clone()).collect::<HashSet<_>>().into_iter().collect();
    let results = fx.engine.set_active(&hashes, true).unwrap();
    assert!(results.iter().all(|r| r.ok), "{results:?}");
    assert_eq!(fx.os.registered(), HashSet::from([path.clone()]), "the file is registered");
    assert_eq!(fx.os.activations_of(&path), 1, "the file is handed to the OS once, not once per face");
    assert!(all_faces(&fx).into_iter().filter(|f| Path::new(&f.path) == path).all(|f| f.active), "every face shows as on");

    fx.engine.set_active(&hashes, false).unwrap();
    assert!(fx.os.registered().is_empty());
}

#[test]
fn variable_font_reports_its_axes() {
    let fx = fixture!();
    let Some((name, bytes)) = first_system_font("variable", &["bahnschrift.ttf", "SegUIVar.ttf"]) else { return };
    let info = typeshelf_core::parse_faces(&bytes).unwrap();
    assert!(info[0].is_variable && !info[0].axes.is_empty(), "{name} should be variable");
    fx.put(&name, &bytes);
    fx.engine.add_folder(&fx.root).unwrap();

    let family = fx.engine.list_families(&FamilyQuery::default()).unwrap().into_iter().find(|f| f.faces.iter().any(|x| x.is_variable)).expect("a variable family");
    assert!(family.is_variable);
    let detail = fx.engine.get_family(&family.key).unwrap();
    let tags: Vec<&str> = detail.axes.iter().map(|a| a.tag.as_str()).collect();
    let want: Vec<&str> = info[0].axes.iter().map(|a| a.tag.as_str()).collect();
    assert_eq!(tags, want);
    assert!(tags.contains(&"wght"), "{tags:?}");
    assert!(detail.axes.iter().all(|a| a.min <= a.default && a.default <= a.max));
}

fn setup_woff2() -> Option<(String, Vec<u8>)> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/setup/ui/fonts/geist-latin-400-normal.woff2");
    std::fs::read(&p).ok().map(|b| ("geist-latin-400-normal.woff2".to_string(), b))
}

#[test]
fn woff2_is_kept_as_a_file_without_faces_and_organize_sets_it_aside() {
    let fx = fixture!();
    let Some((name, bytes)) = setup_woff2() else {
        eprintln!("woff2: sample not found, skipping");
        return;
    };
    let path = fx.put(&name, &bytes);
    fx.engine.add_folder(&fx.root).unwrap();

    // Not a desktop font: no family, nothing to turn on, not counted as a font of the folder.
    assert!(all_faces(&fx).is_empty());
    assert_eq!(fx.engine.list_folders().unwrap().iter().find(|f| Path::new(&f.path) == fx.root).unwrap().font_count, 0);

    // The organizer moves it to "Web fonts" instead of quarantining it as unreadable.
    let plan = fx.engine.organize_plan(false).unwrap();
    assert_eq!(plan.changes.len(), 1, "{:?}", plan.changes);
    match &plan.changes[0].change.action {
        typeshelf_core::organize::Action::MoveAside { from, to } => {
            assert_eq!(from, &path);
            assert!(to.starts_with(fx.data.join("Web fonts")), "{to:?}");
        }
        other => panic!("expected MoveAside, got {other:?}"),
    }
    assert_eq!(plan.changes[0].change.case, typeshelf_core::organize::Case::OtherFormat);
}
