//! "Fonts in a PDF": font dictionaries → PostScript names, matched against the library.

use crate::activation::version_key;
use crate::{Engine, EngineError, PdfFont, PdfFontStatus, Result};
use lopdf::{Dictionary, Document};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// "ABCDEF+Montserrat-Bold" → "Montserrat-Bold".
pub(crate) fn strip_subset(name: &str) -> &str {
    match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

/// Version of the embedded font program, when it is a parseable TrueType/OpenType font.
fn embedded_version(doc: &Document, font: &Dictionary) -> Option<String> {
    // Type0 fonts keep the descriptor on their descendant font.
    let font = match font.get_deref(b"DescendantFonts", doc) {
        Ok(d) => doc.dereference(d.as_array().ok()?.first()?).ok()?.1.as_dict().ok()?,
        Err(_) => font,
    };
    let desc = font.get_deref(b"FontDescriptor", doc).ok()?.as_dict().ok()?;
    [b"FontFile2".as_slice(), b"FontFile3", b"FontFile"].iter().find_map(|key| {
        let stream = desc.get_deref(key, doc).ok()?.as_stream().ok()?;
        let bytes = stream.decompressed_content().unwrap_or_else(|_| stream.content.clone());
        typeshelf_core::parse_faces(&bytes).ok().map(|f| f[0].version.clone())
    })
}

/// PostScript name → (pages it appears on, embedded version).
pub(crate) type PdfUses = BTreeMap<String, (BTreeSet<u32>, Option<String>)>;

pub(crate) fn read_fonts(path: &Path) -> Result<PdfUses> {
    let doc = Document::load(path).map_err(|e| EngineError::new("invalid_pdf", e.to_string()))?;
    let mut fonts = PdfUses::new();
    for (page, id) in doc.get_pages() {
        let Ok(dicts) = doc.get_page_fonts(id) else { continue };
        for dict in dicts.values() {
            let Some(base) = dict.get(b"BaseFont").ok().and_then(|o| o.as_name().ok()) else { continue };
            let name = strip_subset(&String::from_utf8_lossy(base)).to_string();
            let entry = fonts.entry(name).or_default();
            entry.0.insert(page);
            if entry.1.is_none() {
                entry.1 = embedded_version(&doc, dict);
            }
        }
    }
    Ok(fonts)
}

impl Engine {
    pub fn pdf_fonts(&self, path: impl AsRef<Path>) -> Result<Vec<PdfFont>> {
        self.match_fonts(read_fonts(path.as_ref())?)
    }

    /// Looks font names up in the library (shared by the PDF and design-file readers).
    pub(crate) fn match_fonts(&self, fonts: PdfUses) -> Result<Vec<PdfFont>> {
        let google = self.google_names();
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT DISTINCT f.hash, f.version, f.family, f.style, a.hash IS NOT NULL FROM faces f
             LEFT JOIN activations a ON a.hash = f.hash
             WHERE f.ps_name = ?1 COLLATE NOCASE AND EXISTS (SELECT 1 FROM files WHERE hash = f.hash)",
        )?;
        let mut out = Vec::new();
        for (ps, (pages, version)) in fonts {
            // "Arial,Bold" (TrueType style suffix) is looked up as "Arial-Bold" too.
            let mut found: Vec<(String, String, String, String, bool)> = Vec::new();
            for name in [ps.clone(), ps.replace(',', "-")] {
                if found.is_empty() {
                    found = stmt
                        .query_map([&name], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
                        .collect::<rusqlite::Result<_>>()?;
                }
            }
            found.sort_by(|a, b| b.4.cmp(&a.4).then_with(|| version_key(&b.1).cmp(&version_key(&a.1))));
            let family = found.first().map(|f| f.2.clone()).unwrap_or_else(|| ps.split(['-', ',']).next().unwrap_or(&ps).into());
            let in_google = google.contains(&family.to_lowercase().replace(' ', ""));
            let same_version: Vec<&(String, String, String, String, bool)> =
                found.iter().filter(|f| version.as_ref().is_none_or(|v| version_key(v) == version_key(&f.1))).collect();
            let (status, detail, best) = match (found.first(), same_version.first()) {
                (None, _) => (PdfFontStatus::Missing, "Not in your library".to_string(), None),
                (Some(any), None) => (
                    PdfFontStatus::Version,
                    format!("PDF uses version {}, library has {}", version.clone().unwrap_or_default(), any.1),
                    Some(any),
                ),
                (_, Some(m)) if m.4 => (PdfFontStatus::Active, format!("Version {} is active", m.1), Some(*m)),
                (_, Some(m)) => (PdfFontStatus::Inactive, format!("Version {} is in your library", m.1), Some(*m)),
            };
            out.push(PdfFont {
                display: best.map(|b| format!("{} {}", b.2, b.3)).unwrap_or_else(|| ps.clone()),
                ps_name: ps,
                pages: pages.len() as u32,
                status,
                detail,
                hash: best.map(|b| b.0.clone()),
                google: in_google,
            });
        }
        Ok(out)
    }
}
