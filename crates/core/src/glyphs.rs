//! Character coverage from the cmap: compact ranges for glyph search, and the cmap grouped by
//! Unicode block for the glyph grid.

use serde::{Deserialize, Serialize};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::FileRef;
use skrifa::{FontRef, MetadataProvider};

/// Codepoints a face maps, as sorted inclusive `[start, end]` ranges.
pub type Coverage = Vec<[u32; 2]>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlyphBlock {
    pub name: String,
    pub start: u32,
    pub end: u32,
    pub chars: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlyphMap {
    pub total: u32,
    pub blocks: Vec<GlyphBlock>,
}

/// Face `index` of a font file or collection.
pub(crate) fn face_at(bytes: &[u8], index: u32) -> Option<FontRef<'_>> {
    match FileRef::new(bytes).ok()? {
        FileRef::Font(f) if index == 0 => Some(f),
        FileRef::Font(_) => None,
        FileRef::Collection(c) => c.get(index).ok(),
    }
}

/// Sorted, deduplicated codepoints the face maps, control characters left out.
fn codepoints(font: &FontRef) -> Vec<u32> {
    let mut cps: Vec<u32> = font
        .charmap()
        .mappings()
        .map(|(cp, _)| cp)
        .filter(|&cp| char::from_u32(cp).is_some_and(|c| !c.is_control()))
        .collect();
    cps.sort_unstable();
    cps.dedup();
    cps
}

/// Coverage of face `index`; empty when the file does not parse.
pub fn coverage(bytes: &[u8], index: u32) -> Coverage {
    let Some(font) = face_at(bytes, index) else { return Vec::new() };
    let mut out: Coverage = Vec::new();
    for cp in codepoints(&font) {
        match out.last_mut() {
            Some(r) if r[1] + 1 == cp => r[1] = cp,
            _ => out.push([cp, cp]),
        }
    }
    out
}

pub fn covers(coverage: &[[u32; 2]], cp: u32) -> bool {
    let i = coverage.partition_point(|r| r[1] < cp);
    coverage.get(i).is_some_and(|r| r[0] <= cp)
}

/// The cmap of face `index` grouped by Unicode block, in codepoint order; codepoints outside the
/// table go to a last "Other" block.
pub fn glyph_map(bytes: &[u8], index: u32) -> Option<GlyphMap> {
    let font = face_at(bytes, index)?;
    let cps = codepoints(&font);
    let mut blocks: Vec<GlyphBlock> = Vec::new();
    let mut other: Vec<u32> = Vec::new();
    for cp in &cps {
        let i = BLOCKS.partition_point(|b| b.1 < *cp);
        match BLOCKS.get(i).filter(|b| b.0 <= *cp) {
            Some(&(start, end, name)) => match blocks.last_mut() {
                Some(b) if b.start == start => b.chars.push(*cp),
                _ => blocks.push(GlyphBlock { name: name.into(), start, end, chars: vec![*cp] }),
            },
            None => other.push(*cp),
        }
    }
    if let (Some(&start), Some(&end)) = (other.first(), other.last()) {
        blocks.push(GlyphBlock { name: "Other".into(), start, end, chars: other });
    }
    Some(GlyphMap { total: cps.len() as u32, blocks })
}

/// SVG path data with y negated (fonts are y-up, SVG is y-down), plus the font-space box of every
/// point drawn. Control points count too, so the box can be a little larger than the ink: fine for a viewBox.
struct SvgPath {
    d: String,
    /// min x, min y, max x, max y in font units (y-up).
    bounds: [f32; 4],
}

impl SvgPath {
    fn add(&mut self, cmd: char, pts: &[(f32, f32)]) {
        self.d.push(cmd);
        for (i, &(x, y)) in pts.iter().enumerate() {
            let b = &mut self.bounds;
            (b[0], b[1], b[2], b[3]) = (b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y));
            self.d += &format!("{}{x},{}", if i > 0 { " " } else { "" }, -y);
        }
    }
}

impl OutlinePen for SvgPath {
    fn move_to(&mut self, x: f32, y: f32) {
        self.add('M', &[(x, y)]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.add('L', &[(x, y)]);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.add('Q', &[(cx0, cy0), (x, y)]);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.add('C', &[(cx0, cy0), (cx1, cy1), (x, y)]);
    }
    fn close(&mut self) {
        self.d.push('Z');
    }
}

/// Codepoint `cp` of face `index` as a standalone SVG in font units (default instance, unhinted):
/// the viewBox spans 0 to the advance width and ascent to descent, widened to the outline where it
/// reaches past them (zero-advance combining marks, negative side bearings, italic overhang); the baseline is y = 0.
/// `None` when the file does not parse or the face does not map `cp`.
pub fn glyph_svg(bytes: &[u8], index: u32, cp: u32) -> Option<String> {
    let font = face_at(bytes, index)?;
    let gid = font.charmap().map(cp)?;
    let glyph = font.outline_glyphs().get(gid)?;
    let m = font.metrics(Size::unscaled(), LocationRef::default());
    let advance = font.glyph_metrics(Size::unscaled(), LocationRef::default()).advance_width(gid).unwrap_or(0.0);
    // Start from the advance box; drawing widens it to every point of the outline.
    let mut path = SvgPath { d: String::new(), bounds: [0.0, m.descent, advance, m.ascent] };
    glyph.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut path).ok()?;
    let [x0, y0, x1, y1] = path.bounds;
    Some(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x0} {} {} {}"><path d="{}"/></svg>"#, -y1, (x1 - x0).max(1.0), (y1 - y0).max(1.0), path.d))
}

// ponytail: the common blocks only (Unicode 15 Blocks.txt subset); the rest lands in "Other".
// Add rows from Blocks.txt if a script people use shows up there.
const BLOCKS: &[(u32, u32, &str)] = &[
    (0x0000, 0x007F, "Basic Latin"),
    (0x0080, 0x00FF, "Latin-1 Supplement"),
    (0x0100, 0x017F, "Latin Extended-A"),
    (0x0180, 0x024F, "Latin Extended-B"),
    (0x0250, 0x02AF, "IPA Extensions"),
    (0x02B0, 0x02FF, "Spacing Modifier Letters"),
    (0x0300, 0x036F, "Combining Diacritical Marks"),
    (0x0370, 0x03FF, "Greek and Coptic"),
    (0x0400, 0x04FF, "Cyrillic"),
    (0x0500, 0x052F, "Cyrillic Supplement"),
    (0x0530, 0x058F, "Armenian"),
    (0x0590, 0x05FF, "Hebrew"),
    (0x0600, 0x06FF, "Arabic"),
    (0x0700, 0x074F, "Syriac"),
    (0x0750, 0x077F, "Arabic Supplement"),
    (0x0780, 0x07BF, "Thaana"),
    (0x07C0, 0x07FF, "NKo"),
    (0x0800, 0x083F, "Samaritan"),
    (0x08A0, 0x08FF, "Arabic Extended-A"),
    (0x0900, 0x097F, "Devanagari"),
    (0x0980, 0x09FF, "Bengali"),
    (0x0A00, 0x0A7F, "Gurmukhi"),
    (0x0A80, 0x0AFF, "Gujarati"),
    (0x0B00, 0x0B7F, "Oriya"),
    (0x0B80, 0x0BFF, "Tamil"),
    (0x0C00, 0x0C7F, "Telugu"),
    (0x0C80, 0x0CFF, "Kannada"),
    (0x0D00, 0x0D7F, "Malayalam"),
    (0x0D80, 0x0DFF, "Sinhala"),
    (0x0E00, 0x0E7F, "Thai"),
    (0x0E80, 0x0EFF, "Lao"),
    (0x0F00, 0x0FFF, "Tibetan"),
    (0x1000, 0x109F, "Myanmar"),
    (0x10A0, 0x10FF, "Georgian"),
    (0x1100, 0x11FF, "Hangul Jamo"),
    (0x1200, 0x137F, "Ethiopic"),
    (0x13A0, 0x13FF, "Cherokee"),
    (0x1400, 0x167F, "Unified Canadian Aboriginal Syllabics"),
    (0x1680, 0x169F, "Ogham"),
    (0x16A0, 0x16FF, "Runic"),
    (0x1700, 0x171F, "Tagalog"),
    (0x1780, 0x17FF, "Khmer"),
    (0x1800, 0x18AF, "Mongolian"),
    (0x1AB0, 0x1AFF, "Combining Diacritical Marks Extended"),
    (0x1B00, 0x1B7F, "Balinese"),
    (0x1C80, 0x1C8F, "Cyrillic Extended-C"),
    (0x1C90, 0x1CBF, "Georgian Extended"),
    (0x1CD0, 0x1CFF, "Vedic Extensions"),
    (0x1D00, 0x1D7F, "Phonetic Extensions"),
    (0x1D80, 0x1DBF, "Phonetic Extensions Supplement"),
    (0x1DC0, 0x1DFF, "Combining Diacritical Marks Supplement"),
    (0x1E00, 0x1EFF, "Latin Extended Additional"),
    (0x1F00, 0x1FFF, "Greek Extended"),
    (0x2000, 0x206F, "General Punctuation"),
    (0x2070, 0x209F, "Superscripts and Subscripts"),
    (0x20A0, 0x20CF, "Currency Symbols"),
    (0x20D0, 0x20FF, "Combining Diacritical Marks for Symbols"),
    (0x2100, 0x214F, "Letterlike Symbols"),
    (0x2150, 0x218F, "Number Forms"),
    (0x2190, 0x21FF, "Arrows"),
    (0x2200, 0x22FF, "Mathematical Operators"),
    (0x2300, 0x23FF, "Miscellaneous Technical"),
    (0x2400, 0x243F, "Control Pictures"),
    (0x2440, 0x245F, "Optical Character Recognition"),
    (0x2460, 0x24FF, "Enclosed Alphanumerics"),
    (0x2500, 0x257F, "Box Drawing"),
    (0x2580, 0x259F, "Block Elements"),
    (0x25A0, 0x25FF, "Geometric Shapes"),
    (0x2600, 0x26FF, "Miscellaneous Symbols"),
    (0x2700, 0x27BF, "Dingbats"),
    (0x27C0, 0x27EF, "Miscellaneous Mathematical Symbols-A"),
    (0x27F0, 0x27FF, "Supplemental Arrows-A"),
    (0x2800, 0x28FF, "Braille Patterns"),
    (0x2900, 0x297F, "Supplemental Arrows-B"),
    (0x2980, 0x29FF, "Miscellaneous Mathematical Symbols-B"),
    (0x2A00, 0x2AFF, "Supplemental Mathematical Operators"),
    (0x2B00, 0x2BFF, "Miscellaneous Symbols and Arrows"),
    (0x2C00, 0x2C5F, "Glagolitic"),
    (0x2C60, 0x2C7F, "Latin Extended-C"),
    (0x2C80, 0x2CFF, "Coptic"),
    (0x2D00, 0x2D2F, "Georgian Supplement"),
    (0x2D30, 0x2D7F, "Tifinagh"),
    (0x2DE0, 0x2DFF, "Cyrillic Extended-A"),
    (0x2E00, 0x2E7F, "Supplemental Punctuation"),
    (0x2E80, 0x2EFF, "CJK Radicals Supplement"),
    (0x2F00, 0x2FDF, "Kangxi Radicals"),
    (0x3000, 0x303F, "CJK Symbols and Punctuation"),
    (0x3040, 0x309F, "Hiragana"),
    (0x30A0, 0x30FF, "Katakana"),
    (0x3100, 0x312F, "Bopomofo"),
    (0x3130, 0x318F, "Hangul Compatibility Jamo"),
    (0x31F0, 0x31FF, "Katakana Phonetic Extensions"),
    (0x3200, 0x32FF, "Enclosed CJK Letters and Months"),
    (0x3300, 0x33FF, "CJK Compatibility"),
    (0x3400, 0x4DBF, "CJK Unified Ideographs Extension A"),
    (0x4DC0, 0x4DFF, "Yijing Hexagram Symbols"),
    (0x4E00, 0x9FFF, "CJK Unified Ideographs"),
    (0xA000, 0xA48F, "Yi Syllables"),
    (0xA640, 0xA69F, "Cyrillic Extended-B"),
    (0xA700, 0xA71F, "Modifier Tone Letters"),
    (0xA720, 0xA7FF, "Latin Extended-D"),
    (0xA8E0, 0xA8FF, "Devanagari Extended"),
    (0xA980, 0xA9DF, "Javanese"),
    (0xAB30, 0xAB6F, "Latin Extended-E"),
    (0xAC00, 0xD7AF, "Hangul Syllables"),
    (0xE000, 0xF8FF, "Private Use Area"),
    (0xF900, 0xFAFF, "CJK Compatibility Ideographs"),
    (0xFB00, 0xFB4F, "Alphabetic Presentation Forms"),
    (0xFB50, 0xFDFF, "Arabic Presentation Forms-A"),
    (0xFE00, 0xFE0F, "Variation Selectors"),
    (0xFE10, 0xFE1F, "Vertical Forms"),
    (0xFE20, 0xFE2F, "Combining Half Marks"),
    (0xFE30, 0xFE4F, "CJK Compatibility Forms"),
    (0xFE50, 0xFE6F, "Small Form Variants"),
    (0xFE70, 0xFEFF, "Arabic Presentation Forms-B"),
    (0xFF00, 0xFFEF, "Halfwidth and Fullwidth Forms"),
    (0xFFF0, 0xFFFF, "Specials"),
    (0x1D400, 0x1D7FF, "Mathematical Alphanumeric Symbols"),
    (0x1F000, 0x1F02F, "Mahjong Tiles"),
    (0x1F0A0, 0x1F0FF, "Playing Cards"),
    (0x1F100, 0x1F1FF, "Enclosed Alphanumeric Supplement"),
    (0x1F200, 0x1F2FF, "Enclosed Ideographic Supplement"),
    (0x1F300, 0x1F5FF, "Miscellaneous Symbols and Pictographs"),
    (0x1F600, 0x1F64F, "Emoticons"),
    (0x1F680, 0x1F6FF, "Transport and Map Symbols"),
    (0x1F700, 0x1F77F, "Alchemical Symbols"),
    (0x1F780, 0x1F7FF, "Geometric Shapes Extended"),
    (0x1F800, 0x1F8FF, "Supplemental Arrows-C"),
    (0x1F900, 0x1F9FF, "Supplemental Symbols and Pictographs"),
    (0x1FA70, 0x1FAFF, "Symbols and Pictographs Extended-A"),
    (0x20000, 0x2A6DF, "CJK Unified Ideographs Extension B"),
    (0xF0000, 0xFFFFF, "Supplementary Private Use Area-A"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_are_sorted_and_disjoint() {
        assert!(BLOCKS.windows(2).all(|w| w[0].1 < w[1].0 && w[0].0 <= w[0].1));
    }

    #[test]
    fn covers_ranges() {
        let c = vec![[0x20, 0x7E], [0xA0, 0xFF]];
        assert!(covers(&c, 'A' as u32) && covers(&c, 0xFF) && covers(&c, 0x20));
        assert!(!covers(&c, 0x7F) && !covers(&c, 0x100) && !covers(&[], 0x41));
    }

    #[test]
    fn arial_cmap() {
        let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\arial.ttf") else { return };
        let cov = coverage(&bytes, 0);
        assert!(covers(&cov, 'A' as u32) && covers(&cov, 'é' as u32) && !covers(&cov, '中' as u32));
        let map = glyph_map(&bytes, 0).unwrap();
        assert_eq!(map.blocks[0].name, "Basic Latin");
        assert_eq!(map.total as usize, map.blocks.iter().map(|b| b.chars.len()).sum::<usize>());
        assert!(map.blocks.windows(2).all(|w| w[0].start < w[1].start || w[1].name == "Other"));
        assert!(glyph_map(&bytes, 1).is_none());
    }

    #[test]
    fn arial_glyph_svg() {
        let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\arial.ttf") else { return };
        let svg = glyph_svg(&bytes, 0, 'A' as u32).unwrap();
        let attr = |name: &str| svg.split(&format!(" {name}=\"")).nth(1).unwrap().split('"').next().unwrap().to_string();
        // y from -ascent to -descent (font units, flipped to y-down); x covers the advance (1366) widened to
        // the outline, which reaches 3 units left of the origin and 3 past the advance in Arial's "A".
        let vb: Vec<f32> = attr("viewBox").split(' ').map(|v| v.parse().unwrap()).collect();
        assert_eq!(vb, [-3.0, -1854.0, 1372.0, 2288.0]);
        let d = attr("d");
        assert!(d.starts_with('M') && d.ends_with('Z') && d.len() > 20, "{d}");
        // The apex of "A" sits at cap height (1466 units up), so above the baseline in SVG space.
        assert!(d.contains(",-1466"), "{d}");
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\"") && svg.ends_with("</svg>"));
        assert!(glyph_svg(&bytes, 0, '中' as u32).is_none(), "not in the cmap");
        assert!(glyph_svg(&bytes, 1, 'A' as u32).is_none(), "no second face");
    }

    /// An empty glyph with advance 0 (a zero-width joiner, say) still gets a viewBox with a width.
    #[test]
    fn empty_zero_advance_svg_has_a_size() {
        let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\arial.ttf") else { return };
        let font = face_at(&bytes, 0).unwrap();
        let empty: Vec<String> = font.charmap().mappings().filter_map(|(cp, _)| glyph_svg(&bytes, 0, cp)).filter(|s| s.contains(" d=\"\"")).collect();
        assert!(!empty.is_empty(), "Arial has empty glyphs (space, joiners)");
        for svg in empty {
            let vb: Vec<f32> = svg.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap().split(' ').map(|v| v.parse().unwrap()).collect();
            assert!(vb[2] >= 1.0 && vb[3] >= 1.0, "{svg}");
        }
    }

    /// A combining acute has advance 0 and is drawn left of its origin: the viewBox still holds the outline.
    #[test]
    fn combining_mark_svg_is_not_empty() {
        let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\arial.ttf") else { return };
        let Some(svg) = glyph_svg(&bytes, 0, 0x301) else { return };
        let vb: Vec<f32> = svg.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap().split(' ').map(|v| v.parse().unwrap()).collect();
        assert!(vb[2] > 100.0 && vb[3] > 1000.0, "{vb:?}");
        // Every point of the path lies inside the viewBox.
        let d = svg.split(" d=\"").nth(1).unwrap().split('"').next().unwrap();
        let nums: Vec<f32> = d.split(|c: char| c.is_ascii_alphabetic() || c == ' ').flat_map(|p| p.split(',')).filter(|s| !s.is_empty()).map(|s| s.parse().unwrap()).collect();
        for xy in nums.chunks(2) {
            assert!(xy[0] >= vb[0] && xy[0] <= vb[0] + vb[2] && xy[1] >= vb[1] && xy[1] <= vb[1] + vb[3], "{xy:?} outside {vb:?}");
        }
    }
}
