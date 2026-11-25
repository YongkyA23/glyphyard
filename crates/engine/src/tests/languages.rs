//! Language filter: `FamilyQuery::lang`, `lang:<id>` in SuperSearch and `language_counts`.

use super::*;
use typeshelf_core::glyphs::coverage;
use typeshelf_core::languages::language;

/// A decorative system font that has A-Z but no Vietnamese tone marks: file name, bytes, family.
fn without_vietnamese() -> Option<(String, Vec<u8>, String)> {
    let vi = language("vietnamese").unwrap();
    ["ALGER.TTF", "BRUSHSCI.TTF", "CHILLER.TTF", "BAUHS93.TTF", "COLONNA.TTF", "BRADHITC.TTF", "BROADW.TTF"].iter().find_map(|f| {
        let bytes = system_font(f)?;
        let face = typeshelf_core::parse_faces(&bytes).ok()?.into_iter().next()?;
        let cov = coverage(&bytes, face.index);
        (language("indonesian")?.covered_by(&cov) && !vi.covered_by(&cov)).then(|| (f.to_string(), bytes, face.family))
    })
}

fn names(fx: &Fixture, q: FamilyQuery) -> Vec<String> {
    fx.engine.list_families(&q).unwrap().into_iter().map(|f| f.family).collect()
}

#[test]
fn language_filter_by_query_and_supersearch() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    let other = without_vietnamese();
    if let Some((file, bytes, _)) = &other {
        fx.put(file, bytes);
    }
    fx.engine.add_folder(&fx.root).unwrap();

    let lang = |l: &str| names(&fx, FamilyQuery { lang: Some(l.into()), ..Default::default() });
    let search = |s: &str| names(&fx, FamilyQuery { search: Some(s.into()), ..Default::default() });
    assert!(lang("indonesian").contains(&"Arial".into()));
    assert!(lang("vietnamese").contains(&"Arial".into()));
    assert!(!lang("chinese").contains(&"Arial".into()));
    assert_eq!(search("lang:vietnamese"), lang("vietnamese"));
    assert_eq!(search("LANG:Indonesian"), lang("indonesian"));
    if let Some((_, _, family)) = &other {
        assert!(lang("indonesian").contains(family));
        assert!(!lang("vietnamese").contains(family), "no tone marks");
        assert!(search("lang:vietnamese").iter().all(|f| f != family));
    }

    // Composes with the other filters and with paging.
    assert_eq!(search("lang:vietnamese arial"), vec!["Arial".to_string()]);
    assert!(search("lang:chinese arial").is_empty());
    let q = FamilyQuery { lang: Some("vietnamese".into()), search: Some("arial".into()), ..Default::default() };
    let page = fx.engine.family_page(&q, 0, 1).unwrap();
    assert_eq!((page.total, page.items.len()), (1, 1));
    assert_eq!(fx.engine.family_page(&q, 1, 1).unwrap().items.len(), 0);

    // An unknown id matches nothing (as a query field); as a token it is a plain word.
    assert!(lang("klingon").is_empty());
    assert!(search("lang:klingon").is_empty());
}

#[test]
fn language_counts_list_every_language_with_family_counts() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let counts = fx.engine.language_counts().unwrap();
    assert_eq!(counts.len(), typeshelf_core::languages::LANGUAGES.len());
    let n = |id: &str| counts.iter().find(|c| c.id == id).unwrap().families;
    assert_eq!((counts[0].id.as_str(), counts[0].name.as_str()), ("english", "English"));
    assert_eq!((n("english"), n("indonesian"), n("vietnamese"), n("chinese")), (1, 1, 1, 0));
    for c in &counts {
        let q = FamilyQuery { lang: Some(c.id.clone()), ..Default::default() };
        assert_eq!(fx.engine.list_families(&q).unwrap().len(), c.families, "{} matches the filter", c.id);
    }
}
