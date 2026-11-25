//! Family page glyph tools: a glyph as SVG and the per-face metrics behind the anatomy overlay.

use super::*;

#[test]
fn glyph_svg_and_face_metrics() {
    let fx = fixture!();
    let Some(bytes) = system_font("arial.ttf") else { return };
    fx.put("arial.ttf", &bytes);
    fx.engine.add_folder(&fx.root).unwrap();
    let arial = fx.family("Arial");
    let hash = &arial.faces[0].hash;

    let svg = fx.engine.glyph_svg(hash, 0, 'A' as u32).unwrap();
    assert!(svg.contains("viewBox=\"-3 -1854 1372 2288\"") && svg.contains("<path d=\"M"), "{svg}");
    assert_eq!(fx.engine.glyph_svg(hash, 0, '中' as u32).unwrap_err().code, "not_found");
    assert_eq!(fx.engine.glyph_svg("nope", 0, 'A' as u32).unwrap_err().code, "not_found");

    let detail = fx.engine.get_family(&arial.key).unwrap();
    let m = &detail.face_metrics[hash];
    assert!(m.x_height > 0.5 && m.x_height < 0.55 && m.cap_height > 0.7 && m.cap_height < 0.73, "{m:?}");
    let json = serde_json::to_value(&detail).unwrap();
    assert!(json["faceMetrics"][hash]["x_height"].is_number());
    // Arial: hhea ascent 1854, descent -434 over 2048 units per em.
    assert!((m.ascender - 1854.0 / 2048.0).abs() < 0.001 && (m.descender + 434.0 / 2048.0).abs() < 0.001, "{m:?}");
}

/// `metrics_json` rows stored before ascender/descender existed still parse, with those two at 0 (unknown).
#[test]
fn old_metrics_json_still_parses() {
    let old = r#"{"category":"sans","x_height":0.5,"cap_height":0.7,"avg_width":0.4,"width_class":5,"panose":[2,11,0,0,0,0,0,0,0,0],"fixed_pitch":false}"#;
    let m: Metrics = serde_json::from_str(old).unwrap();
    assert_eq!((m.x_height, m.cap_height, m.ascender, m.descender), (0.5, 0.7, 0.0, 0.0));
}
