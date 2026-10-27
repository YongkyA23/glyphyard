use serde::{Deserialize, Serialize};
use skrifa::raw::{FileRef, TableProvider};
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Axis {
    pub tag: String,
    pub min: f32,
    pub default: f32,
    pub max: f32,
}

/// Metadata of one face. A TTC/OTC file yields one per face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceInfo {
    pub index: u32,
    pub ps_name: String,
    pub family: String,
    pub style: String,
    pub full_name: String,
    /// Normalized version like "2.005", from the name table, falling back to head.fontRevision.
    pub version: String,
    /// OS/2 vendor id, or the manufacturer name when the vendor id is blank.
    pub foundry: String,
    pub designer: String,
    pub weight: f32,
    pub italic: bool,
    pub is_variable: bool,
    pub axes: Vec<Axis>,
    /// OpenType feature tags from GSUB and GPOS, sorted, deduplicated.
    pub features: Vec<String>,
    pub glyph_count: u32,
}

#[derive(Debug, PartialEq)]
pub enum ParseError {
    NotAFont,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not a valid font file")
    }
}

impl std::error::Error for ParseError {}

pub fn parse_faces(bytes: &[u8]) -> Result<Vec<FaceInfo>, ParseError> {
    let file = FileRef::new(bytes).map_err(|_| ParseError::NotAFont)?;
    let faces: Vec<FaceInfo> = file
        .fonts()
        .enumerate()
        .filter_map(|(i, f)| f.ok().map(|f| face_info(&f, i as u32)))
        .collect();
    if faces.is_empty() || faces.iter().all(|f| f.ps_name.is_empty() && f.family.is_empty()) {
        return Err(ParseError::NotAFont);
    }
    Ok(faces)
}

fn name(font: &FontRef, id: StringId) -> String {
    font.localized_strings(id)
        .english_or_first()
        .map(|s| s.to_string().trim().to_string())
        .unwrap_or_default()
}

fn face_info(font: &FontRef, index: u32) -> FaceInfo {
    // Typographic names (16/17) win over the legacy 4-style family names (1/2).
    let family = Some(name(font, StringId::TYPOGRAPHIC_FAMILY_NAME))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| name(font, StringId::FAMILY_NAME));
    let style = Some(name(font, StringId::TYPOGRAPHIC_SUBFAMILY_NAME))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| name(font, StringId::SUBFAMILY_NAME));
    let mut ps_name = name(font, StringId::POSTSCRIPT_NAME);
    if ps_name.is_empty() {
        ps_name = format!("{family}-{style}").replace(' ', "");
    }
    let revision = font.head().map(|h| h.font_revision().to_f32()).unwrap_or(0.0);
    let version = normalize_version(&name(font, StringId::VERSION_STRING), revision);

    let vendor = font
        .os2()
        .map(|o| o.ach_vend_id().to_string().trim().to_string())
        .unwrap_or_default();
    let foundry = if vendor.is_empty() || vendor.chars().all(|c| c == '-' || c == '\0') {
        name(font, StringId::MANUFACTURER)
    } else {
        vendor
    };

    let attrs = font.attributes();
    let axes: Vec<Axis> = font
        .axes()
        .iter()
        .map(|a| Axis {
            tag: a.tag().to_string(),
            min: a.min_value(),
            default: a.default_value(),
            max: a.max_value(),
        })
        .collect();

    let mut features: Vec<String> = Vec::new();
    if let Ok(list) = font.gsub().and_then(|t| t.feature_list()) {
        features.extend(list.feature_records().iter().map(|r| r.feature_tag().to_string()));
    }
    if let Ok(list) = font.gpos().and_then(|t| t.feature_list()) {
        features.extend(list.feature_records().iter().map(|r| r.feature_tag().to_string()));
    }
    features.sort();
    features.dedup();

    FaceInfo {
        index,
        full_name: name(font, StringId::FULL_NAME),
        designer: name(font, StringId::DESIGNER),
        weight: attrs.weight.value(),
        italic: !matches!(attrs.style, skrifa::attribute::Style::Normal),
        is_variable: !axes.is_empty(),
        axes,
        features,
        glyph_count: font.maxp().map(|m| m.num_glyphs() as u32).unwrap_or(0),
        ps_name,
        family,
        style,
        version,
        foundry,
    }
}

/// "Version 2.005;PS 002.005;hotconv" → "2.005". Falls back to head.fontRevision.
pub fn normalize_version(name_version: &str, revision: f32) -> String {
    let digits: String = name_version
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let digits = digits.trim_end_matches('.');
    if !digits.is_empty() {
        return digits.to_string();
    }
    format!("{revision:.3}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_from_name_table() {
        assert_eq!(normalize_version("Version 2.005;PS 002.005;hotconv 1.0.88", 0.0), "2.005");
        assert_eq!(normalize_version("1.10", 0.0), "1.10");
        assert_eq!(normalize_version("", 1.5), "1.500");
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_faces(b"definitely not a font"), Err(ParseError::NotAFont));
    }

    /// Parses a font that ships with the OS, so the test needs no fixture files.
    #[test]
    fn parses_a_system_font() {
        let candidates = [
            "C:\\Windows\\Fonts\\arial.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ];
        let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) else {
            eprintln!("no system font found, skipping");
            return;
        };
        let faces = parse_faces(&bytes).expect("system font parses");
        let f = &faces[0];
        assert!(!f.ps_name.is_empty());
        assert!(!f.family.is_empty());
        assert!(f.glyph_count > 100);
        assert!(f.weight >= 100.0);
    }
}
