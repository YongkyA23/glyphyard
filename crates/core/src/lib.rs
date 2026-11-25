//! Pure logic shared by the desktop app and the server: font parsing, hashing,
//! the auto-organize planner and the name-change rule. No I/O beyond reading bytes.

pub mod face;
pub mod glyphs;
pub mod languages;
pub mod name_rule;
pub mod organize;
pub mod update;

pub use face::{font_details, parse_faces, Axis, Category, FaceInfo, FontDetails, Metrics, ParseError};
pub use glyphs::{GlyphBlock, GlyphMap};

/// Content identity of a font file. Two files are "the same font file" only if this matches.
pub fn file_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
