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
    assert!(svg.contains("viewBox=\"0 -1854 1366 2288\"") && svg.contains("<path d=\"M"), "{svg}");
    assert_eq!(fx.engine.glyph_svg(hash, 0, '中' as u32).unwrap_err().code, "not_found");
    assert_eq!(fx.engine.glyph_svg("nope", 0, 'A' as u32).unwrap_err().code, "not_found");

    let detail = fx.engine.get_family(&arial.key).unwrap();
    let m = &detail.face_metrics[hash];
    assert!(m.x_height > 0.5 && m.x_height < 0.55 && m.cap_height > 0.7 && m.cap_height < 0.73, "{m:?}");
    let json = serde_json::to_value(&detail).unwrap();
    assert!(json["faceMetrics"][hash]["x_height"].is_number());
}
