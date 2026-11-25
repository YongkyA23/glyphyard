//! Playground: pairing suggestions, x-height in the family detail, variable axis presets.

use super::*;
use std::collections::BTreeMap;

fn x_height(file: &str) -> f32 {
    typeshelf_core::parse_faces(&system_font(file).unwrap()).unwrap()[0].metrics.x_height
}

#[test]
fn pairing_suggests_other_categories_by_x_height() {
    let fx = fixture!();
    if !put_mixed(&fx) {
        return;
    }
    // Arial is a sans: Verdana (sans) is left out, Times (serif) and Consolas (mono) are in,
    // nearest x-height first.
    let arial = fx.family("Arial");
    let got: Vec<String> = fx.engine.pairing_suggestions(&arial.key).unwrap().into_iter().map(|f| f.family).collect();
    let xa = x_height("arial.ttf");
    let mut want = [("Times New Roman", (x_height("times.ttf") - xa).abs()), ("Consolas", (x_height("consola.ttf") - xa).abs())];
    want.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(got, want.iter().map(|w| w.0.to_string()).collect::<Vec<_>>());

    // Times (serif) gets every sans and the mono, never itself.
    let times = fx.family("Times New Roman");
    let got: Vec<String> = fx.engine.pairing_suggestions(&times.key).unwrap().into_iter().map(|f| f.family).collect();
    assert!(got.contains(&"Arial".to_string()) && got.contains(&"Consolas".to_string()));
    assert!(!got.contains(&"Times New Roman".to_string()));
    assert!(got.len() <= 8);

    assert_eq!(fx.engine.pairing_suggestions("nope").unwrap_err().code, "not_found");
}

#[test]
fn family_detail_has_the_stored_x_height() {
    let fx = fixture!();
    if !put_mixed(&fx) {
        return;
    }
    let detail = fx.engine.get_family(&fx.family("Arial").key).unwrap();
    assert!((detail.x_height - x_height("arial.ttf")).abs() < 1e-4);
    assert!(detail.x_height > 0.3 && detail.x_height < 0.7);
}

#[test]
fn axis_presets_save_list_replace_delete() {
    let fx = fixture!();
    let vals = |w: f32| BTreeMap::from([("wght".to_string(), w), ("wdth".to_string(), 90.0)]);
    assert!(fx.engine.axis_presets("fam").unwrap().is_empty());
    fx.engine.save_axis_preset("fam", " Heavy ", &vals(800.0)).unwrap();
    let list = fx.engine.save_axis_preset("fam", "Light", &vals(300.0)).unwrap();
    assert_eq!(list.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Heavy", "Light"]);
    // The same name replaces the saved values.
    let list = fx.engine.save_axis_preset("fam", "Heavy", &vals(900.0)).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].values["wght"], 900.0);
    // Per family.
    assert!(fx.engine.axis_presets("other").unwrap().is_empty());
    assert_eq!(fx.engine.save_axis_preset("fam", "  ", &vals(1.0)).unwrap_err().code, "invalid");
    let list = fx.engine.delete_axis_preset("fam", "Heavy").unwrap();
    assert_eq!(list.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Light"]);
    assert_eq!(fx.engine.axis_presets("fam").unwrap(), list);
}
