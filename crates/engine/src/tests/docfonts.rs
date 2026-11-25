use super::*;

#[test]
fn ai_file_fonts_match_by_postscript_name() {
    let fx = fixture!();
    fx.put("arial.ttf", &system_font("arial.ttf").unwrap());
    fx.engine.add_folder(&fx.root).unwrap();
    let ai = fx.root.parent().unwrap().join("logo.ai");
    let item = |n: &str| format!("<rdf:li rdf:parseType=\"Resource\"><stFnt:fontName>{n}</stFnt:fontName></rdf:li>");
    std::fs::write(&ai, format!("%!PS-Adobe\n<?xpacket begin=\"\"?><xmpTPg:Fonts><rdf:Bag>{}{}</rdf:Bag></xmpTPg:Fonts><?xpacket end=\"w\"?>", item("ArialMT"), item("Nope-Bold"))).unwrap();

    let rows = fx.engine.document_fonts(&ai).unwrap().fonts;
    assert_eq!(rows.len(), 2);
    let arial = rows.iter().find(|f| f.ps_name == "ArialMT").unwrap();
    assert_eq!((arial.status, arial.hash.is_some()), (PdfFontStatus::Inactive, true));
    fx.engine.set_active(&[arial.hash.clone().unwrap()], true).unwrap();
    let rows = fx.engine.document_fonts(&ai).unwrap().fonts;
    assert_eq!(rows.iter().find(|f| f.ps_name == "ArialMT").unwrap().status, PdfFontStatus::Active);
    let nope = rows.iter().find(|f| f.ps_name == "Nope-Bold").unwrap();
    assert_eq!((nope.status, nope.hash.is_none()), (PdfFontStatus::Missing, true));

    let txt = ai.with_extension("txt");
    std::fs::write(&txt, b"x").unwrap();
    assert!(fx.engine.document_fonts(&txt).is_err());
    // A design file that gives no fonts is an empty, complete list.
    let empty = ai.with_file_name("empty.indd");
    std::fs::write(&empty, b"").unwrap();
    let r = fx.engine.document_fonts(&empty).unwrap();
    assert!(r.fonts.is_empty() && r.partial.is_none());
    assert_eq!(serde_json::to_value(&r).unwrap()["partial"], serde_json::Value::Null);
}
