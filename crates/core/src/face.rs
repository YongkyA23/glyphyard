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
    /// Classification and proportions, for the category filter and "similar fonts".
    #[serde(default)]
    pub metrics: Metrics,
}

/// FontBase's font categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Serif,
    Sans,
    Display,
    Handwriting,
    Mono,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Serif => "serif",
            Category::Sans => "sans",
            Category::Display => "display",
            Category::Handwriting => "handwriting",
            Category::Mono => "mono",
        }
    }

    pub fn parse(s: &str) -> Option<Category> {
        Some(match s {
            "serif" => Category::Serif,
            "sans" => Category::Sans,
            "display" => Category::Display,
            "handwriting" => Category::Handwriting,
            "mono" => Category::Mono,
            _ => return None,
        })
    }
}

/// OS/2 / post / head numbers; lengths are divided by unitsPerEm (0 when the font does not say).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub category: Option<Category>,
    pub x_height: f32,
    pub cap_height: f32,
    pub avg_width: f32,
    /// usWidthClass, 1 (ultra-condensed) to 9 (ultra-expanded); 5 is normal.
    pub width_class: u16,
    pub panose: [u8; 10],
    pub fixed_pitch: bool,
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
        metrics: metrics(font, &family),
        ps_name,
        family,
        style,
        version,
        foundry,
    }
}

fn metrics(font: &FontRef, family: &str) -> Metrics {
    let upm = font.head().map(|h| h.units_per_em()).unwrap_or(0).max(1) as f32;
    let mut m = Metrics {
        fixed_pitch: font.post().is_ok_and(|p| p.is_fixed_pitch() != 0),
        width_class: 5,
        ..Default::default()
    };
    let mut family_class = 0;
    if let Ok(os2) = font.os2() {
        m.x_height = os2.sx_height().unwrap_or(0).max(0) as f32 / upm;
        m.cap_height = os2.s_cap_height().unwrap_or(0).max(0) as f32 / upm;
        m.avg_width = os2.x_avg_char_width().max(0) as f32 / upm;
        m.width_class = os2.us_width_class().clamp(1, 9);
        m.panose.iter_mut().zip(os2.panose_10()).for_each(|(d, s)| *d = *s);
        family_class = (os2.s_family_class() >> 8) as u8;
    }
    m.category = category(m.fixed_pitch, &m.panose, family_class, family);
    m
}

/// post.isFixedPitch, then PANOSE, then the OS/2 IBM family class, then words in the family name.
// ponytail: metadata only, no glyph analysis; fonts with blank PANOSE/class and a plain name stay uncategorized.
pub fn category(fixed_pitch: bool, panose: &[u8; 10], family_class: u8, family: &str) -> Option<Category> {
    if fixed_pitch || (panose[0] == 2 && panose[3] == 9) {
        return Some(Category::Mono);
    }
    match (panose[0], panose[1]) {
        (3, _) => return Some(Category::Handwriting),
        (4 | 5, _) => return Some(Category::Display),
        (2, 11..=15) => return Some(Category::Sans),
        (2, 2..=10) => return Some(Category::Serif),
        _ => {}
    }
    match family_class {
        1..=7 => return Some(Category::Serif),
        8 => return Some(Category::Sans),
        9 | 12 => return Some(Category::Display),
        10 => return Some(Category::Handwriting),
        _ => {}
    }
    let name = family.to_lowercase();
    [("mono", Category::Mono), ("sans", Category::Sans), ("serif", Category::Serif), ("script", Category::Handwriting), ("hand", Category::Handwriting)]
        .into_iter()
        .find(|(word, _)| name.contains(word))
        .map(|(_, c)| c)
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
        assert!(f.metrics.x_height > 0.3 && f.metrics.x_height < 0.7);
    }

    #[test]
    fn categories_from_metadata() {
        let p = |b: [u8; 4]| [b[0], b[1], 0, b[3], 0, 0, 0, 0, 0, 0];
        assert_eq!(category(true, &[0; 10], 0, "X"), Some(Category::Mono));
        assert_eq!(category(false, &p([2, 11, 0, 3]), 0, "Arial"), Some(Category::Sans));
        assert_eq!(category(false, &p([2, 2, 0, 3]), 0, "Times"), Some(Category::Serif));
        assert_eq!(category(false, &p([2, 2, 0, 9]), 0, "Courier"), Some(Category::Mono));
        assert_eq!(category(false, &p([3, 0, 0, 0]), 0, "X"), Some(Category::Handwriting));
        assert_eq!(category(false, &[0; 10], 8, "X"), Some(Category::Sans));
        assert_eq!(category(false, &[0; 10], 0, "Noto Sans Mono"), Some(Category::Mono));
        assert_eq!(category(false, &[0; 10], 0, "Plain"), None);
    }

    #[test]
    fn categories_of_windows_fonts() {
        for (file, want) in [("arial.ttf", Category::Sans), ("times.ttf", Category::Serif), ("consola.ttf", Category::Mono)] {
            let Ok(bytes) = std::fs::read(format!("C:\\Windows\\Fonts\\{file}")) else { continue };
            assert_eq!(parse_faces(&bytes).unwrap()[0].metrics.category, Some(want), "{file}");
        }
    }
}
