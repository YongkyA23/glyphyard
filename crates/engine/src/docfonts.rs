//! "Fonts in a design file": font names from .ai / .indd (XMP packet) and .psd (text engine data),
//! matched against the library with the same rules as a PDF.

use crate::pdf::{read_fonts, PdfUses};
use crate::{Engine, EngineError, PdfFont, Result};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

const CHUNK: usize = 1 << 20;
/// Longest stretch one item may span; the tail of each chunk is re-read with the next, so an item split by
/// a chunk boundary is still found whole.
const OVERLAP: usize = 64 << 10;
// ponytail: scans at most the first 1 GiB and keeps at most 500 names; a larger file is read as far as the cap.
// Raise the cap or seek to the XMP packet at the end of the file if real files need it.
const MAX_BYTES: u64 = 1 << 30;
const MAX_NAMES: usize = 500;
/// Names Adobe apps write as defaults, not fonts anyone chose.
const PLACEHOLDERS: &[&str] = &["AdobeInvisFont"];

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn add(out: &mut BTreeSet<String>, name: &str) {
    let name = name.trim();
    if !name.is_empty() && !PLACEHOLDERS.contains(&name) && out.len() <= MAX_NAMES { // one past the cap, so scan() can tell a name was refused
        out.insert(name.to_string());
    }
}

/// Every complete `<stFnt:fontName>…</stFnt:fontName>` in `buf`.
pub(crate) fn xmp_fonts(buf: &[u8], out: &mut BTreeSet<String>) {
    const OPEN: &[u8] = b"<stFnt:fontName>";
    const CLOSE: &[u8] = b"</stFnt:fontName>";
    let mut at = 0;
    while let Some(start) = find(buf, OPEN, at) {
        let from = start + OPEN.len();
        let window = (from + 1024).min(buf.len());
        match find(&buf[..window], CLOSE, from) {
            // Another opener inside means this one was never closed: look at that one next.
            Some(end) if find(&buf[..end], OPEN, from).is_some() => at = from,
            Some(end) => {
                add(out, &unescape_xml(&String::from_utf8_lossy(&buf[from..end])));
                at = end;
            }
            // Cut off by the end of the buffer (the carried overlap re-reads it) or not a name. Resume at the last
            // opener inside the stretch already searched (an entry may start there and close after it), or after
            // it when there is none, so many unclosed openers cost about one pass, not one search each.
            None => at = rfind(&buf[from..window], OPEN).map_or(window, |q| from + q),
        }
    }
}

/// A PostScript string starting after its `(`: the bytes with `\x` escapes undone, and the index after `)`.
fn ps_string(buf: &[u8], mut i: usize) -> Option<(Vec<u8>, usize)> {
    let mut bytes = Vec::new();
    while let Some(&b) = buf.get(i) {
        match b {
            b')' => return Some((bytes, i + 1)),
            b'\\' => {
                bytes.push(*buf.get(i + 1)?);
                i += 2;
            }
            _ => {
                bytes.push(b);
                i += 1;
            }
        }
    }
    None // unterminated
}

/// UTF-16BE with a byte-order mark (what Photoshop writes); anything else is read as plain text.
fn decode_name(bytes: &[u8]) -> String {
    match bytes {
        [0xFE, 0xFF, rest @ ..] => {
            let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|p| u16::from_be_bytes(*p)).collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// The `/Name (…)` entries of every complete `/FontSet [ … ]` in `buf`.
pub(crate) fn psd_fonts(buf: &[u8], out: &mut BTreeSet<String>) {
    const SET: &[u8] = b"/FontSet [";
    const NAME: &[u8] = b"/Name (";
    let mut at = 0;
    while let Some(start) = find(buf, SET, at) {
        let from = start + SET.len();
        let end = (from + OVERLAP).min(buf.len());
        let mut i = from;
        // Walk the array until its `]`; other strings are skipped whole so a `]` inside one does not end it.
        while i < end {
            match buf[i] {
                b']' => break,
                b'(' => match ps_string(&buf[..end], i + 1) {
                    Some((_, next)) => i = next,
                    None => {
                        i = end; // no `)` anywhere in the window: nothing after this can be read either
                        break;
                    }
                },
                b'/' if buf[i..end].starts_with(NAME) => match ps_string(&buf[..end], i + NAME.len()) {
                    Some((bytes, next)) => {
                        add(out, &decode_name(&bytes));
                        i = next;
                    }
                    None => {
                        i = end;
                        break;
                    }
                },
                _ => i += 1,
            }
        }
        at = i.max(from); // resume past what the walk covered
    }
}

struct Scanned {
    names: BTreeSet<String>,
    /// Why only part of the file was read (size or name cap), in plain words.
    partial: Option<String>,
}

/// What the page shows when a cap or a guard kept us from reading the whole file.
fn join_notes(notes: Vec<String>) -> Option<String> {
    (!notes.is_empty()).then(|| notes.join(" "))
}

/// Runs `extract` over the file in chunks, carrying the tail of each chunk into the next.
fn scan(path: &Path, extract: fn(&[u8], &mut BTreeSet<String>), max_bytes: u64) -> Result<Scanned> {
    let io = |e: std::io::Error| EngineError::new("io", e.to_string());
    let file = std::fs::File::open(path).map_err(io)?;
    let len = file.metadata().map_err(io)?.len();
    let mut reader = file.take(max_bytes);
    let mut out = BTreeSet::new();
    let mut buf: Vec<u8> = Vec::with_capacity(CHUNK + OVERLAP);
    loop {
        if (&mut reader).take(CHUNK as u64).read_to_end(&mut buf).map_err(io)? == 0 {
            break;
        }
        extract(&buf, &mut out);
        if out.len() > MAX_NAMES {
            break;
        }
        buf.drain(..buf.len().saturating_sub(OVERLAP));
    }
    let mut notes = Vec::new();
    if len > max_bytes {
        let size = if max_bytes.is_multiple_of(1 << 30) { format!("{} GiB", max_bytes >> 30) } else { format!("{max_bytes} bytes") };
        notes.push(format!("Only the first {size} of this file was read."));
    }
    if out.len() > MAX_NAMES {
        out.pop_last();
        notes.push(format!("Showing only {MAX_NAMES} of the fonts found."));
    }
    Ok(Scanned { names: out, partial: join_notes(notes) })
}

/// Fonts of a document plus, when the file was only partly read, why.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocFonts {
    pub fonts: Vec<PdfFont>,
    pub partial: Option<String>,
}

/// Above this a PDF-compatible .ai is not also read as a PDF (the PDF reader loads the whole file).
const MAX_PDF_FALLBACK: u64 = 256 << 20;

impl Engine {
    /// Fonts used by a .pdf, .ai, .indd or .psd file, matched against the library.
    pub fn document_fonts(&self, path: impl AsRef<Path>) -> Result<DocFonts> {
        let path = path.as_ref();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let scanned = match ext.as_str() {
            "pdf" => return Ok(DocFonts { fonts: self.pdf_fonts(path)?, partial: None }),
            "psd" => scan(path, psd_fonts, MAX_BYTES)?,
            "indd" => scan(path, xmp_fonts, MAX_BYTES)?,
            "ai" => {
                let mut scanned = scan(path, xmp_fonts, MAX_BYTES)?;
                if scanned.names.is_empty() {
                    // A PDF-compatible .ai without a readable XMP list still has PDF font dictionaries.
                    let size = std::fs::metadata(path).map_err(|e| EngineError::new("io", e.to_string()))?.len();
                    if size > MAX_PDF_FALLBACK {
                        let note = format!("This file is too large (over {} MiB) to also read as a PDF.", MAX_PDF_FALLBACK >> 20);
                        scanned.partial = join_notes(scanned.partial.into_iter().chain([note]).collect());
                    } else {
                        return Ok(DocFonts { fonts: self.match_fonts(read_fonts(path).unwrap_or_default())?, partial: scanned.partial });
                    }
                }
                scanned
            }
            _ => return Err(EngineError::new("unsupported_file", "Choose a .pdf, .psd, .ai or .indd file.")),
        };
        let fonts = self.match_fonts(scanned.names.into_iter().map(|n| (n, Default::default())).collect::<PdfUses>())?;
        Ok(DocFonts { fonts, partial: scanned.partial })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name as Photoshop stores it: UTF-16BE with a byte-order mark, `(` `)` `\` escaped.
    fn utf16(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for b in [0xFE, 0xFF].into_iter().chain(name.encode_utf16().flat_map(|u| u.to_be_bytes())) {
            if matches!(b, b'(' | b')' | b'\\') {
                out.push(b'\\');
            }
            out.push(b);
        }
        out
    }

    fn psd(names: &[&str]) -> Vec<u8> {
        let mut v = b"junk /FontSet [".to_vec();
        for n in names {
            v.extend(b"\n<< /Name (");
            v.extend(utf16(n));
            v.extend(b") /Script 0 /FontType 1 /Synthetic 0 >>");
        }
        v.extend(b"\n] /Other [ (x) ]");
        v
    }

    fn xmp(names: &[&str]) -> Vec<u8> {
        let mut s = String::from("<?xpacket begin=\"\" id=\"W5M0\"?><x:xmpmeta><xmpTPg:Fonts><rdf:Bag>");
        for n in names {
            s += &format!("<rdf:li rdf:parseType=\"Resource\"><stFnt:fontName>{n}</stFnt:fontName><stFnt:fontFamily>F</stFnt:fontFamily><stFnt:fontFace>Regular</stFnt:fontFace></rdf:li>");
        }
        s += "</rdf:Bag></xmpTPg:Fonts></x:xmpmeta><?xpacket end=\"w\"?>";
        s.into_bytes()
    }

    fn run(f: fn(&[u8], &mut BTreeSet<String>), b: &[u8]) -> Vec<String> {
        let mut out = BTreeSet::new();
        f(b, &mut out);
        out.into_iter().collect()
    }

    #[test]
    fn xmp_one_several_none_truncated() {
        assert_eq!(run(xmp_fonts, &xmp(&["ArialMT"])), ["ArialMT"]);
        assert_eq!(run(xmp_fonts, &xmp(&["B-Bold", "A-Regular", "B-Bold"])), ["A-Regular", "B-Bold"], "sorted, once each");
        assert!(run(xmp_fonts, &xmp(&[])).is_empty());
        assert!(run(xmp_fonts, b"").is_empty());
        let mut cut = xmp(&["ArialMT", "Cut-Off"]);
        let at = String::from_utf8_lossy(&cut).find("Cut-O").unwrap() + 3;
        cut.truncate(at);
        assert_eq!(run(xmp_fonts, &cut), ["ArialMT"], "the unfinished name is dropped");
        assert_eq!(run(xmp_fonts, &xmp(&["A&amp;B-Bold"])), ["A&B-Bold"]);
    }

    #[test]
    fn psd_font_set_utf16() {
        assert_eq!(run(psd_fonts, &psd(&["ArialMT", "MyriadPro-Regular", "AdobeInvisFont"])), ["ArialMT", "MyriadPro-Regular"]);
        assert_eq!(run(psd_fonts, &psd(&["Odd(Name)\\X"])), ["Odd(Name)\\X"], "escapes undone");
        assert!(run(psd_fonts, b"nothing here").is_empty());
        assert!(run(psd_fonts, b"/FontSet [ << /Name (\xFE\xFF\x00A").is_empty(), "unterminated string");
        // Odd-length UTF-16 drops the stray byte instead of failing.
        assert_eq!(run(psd_fonts, b"/FontSet [ << /Name (\xFE\xFF\x00A\x00B\x00) >> ]"), ["AB"]);
    }

    #[test]
    fn scanners_stay_linear_on_openers_without_closers() {
        let t = std::time::Instant::now();
        assert!(run(psd_fonts, &b"/FontSet [".repeat(40 << 10)).is_empty());
        assert!(run(psd_fonts, &b"/FontSet [ (".repeat(40 << 10)).is_empty(), "unterminated string");
        assert!(run(psd_fonts, &b"/FontSet [ /Name (".repeat(30 << 10)).is_empty(), "unterminated name");
        assert!(run(xmp_fonts, &b"<stFnt:fontName>".repeat(256 << 10)).is_empty());
        assert!(t.elapsed().as_secs() < 5, "took {:?}", t.elapsed());
    }

    #[test]
    fn xmp_entry_inside_an_unclosed_openers_window_is_kept() {
        let mut v = b"<stFnt:fontName>".to_vec();
        v.extend(vec![b'x'; 1000]);
        v.extend(b"<stFnt:fontName>Real-Font</stFnt:fontName>");
        assert_eq!(run(xmp_fonts, &v), ["Real-Font"]);
    }

    #[test]
    fn scan_finds_markers_split_across_the_chunk_boundary() {
        let dir = std::env::temp_dir().join(format!("ts-docfonts-split-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("split.ai");
        // The marker (or the name inside it) crosses the CHUNK mark; without the carried overlap it is missed.
        for back in [5, 22] {
            let packet = xmp(&["Boundary-Font"]);
            let lead = find(&packet, b"<stFnt:fontName>", 0).unwrap();
            let mut data = vec![b'x'; CHUNK - back - lead];
            data.extend(&packet);
            data.extend(vec![b'y'; CHUNK]);
            std::fs::write(&f, &data).unwrap();
            assert_eq!(scan(&f, xmp_fonts, MAX_BYTES).unwrap().names.into_iter().collect::<Vec<_>>(), ["Boundary-Font"], "xmp, {back} bytes before the mark");
        }
        let mut data = vec![b'x'; CHUNK - 5];
        data.extend(psd(&["Boundary-Font"]));
        data.extend(vec![b'y'; CHUNK]);
        std::fs::write(&f, &data).unwrap();
        assert_eq!(scan(&f, psd_fonts, MAX_BYTES).unwrap().names.into_iter().collect::<Vec<_>>(), ["Boundary-Font"], "psd");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn caps_are_reported() {
        let dir = std::env::temp_dir().join(format!("ts-docfonts-caps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("caps.indd");
        let names: Vec<String> = (0..MAX_NAMES + 5).map(|i| format!("Font-{i}")).collect();
        std::fs::write(&f, xmp(&names.iter().map(String::as_str).collect::<Vec<_>>())).unwrap();
        let r = scan(&f, xmp_fonts, MAX_BYTES).unwrap();
        assert_eq!(r.names.len(), MAX_NAMES);
        assert!(r.partial.unwrap().contains("500"));
        let exact: Vec<String> = (0..MAX_NAMES).map(|i| format!("Font-{i}")).collect();
        std::fs::write(&f, xmp(&exact.iter().map(String::as_str).collect::<Vec<_>>())).unwrap();
        let r = scan(&f, xmp_fonts, MAX_BYTES).unwrap();
        assert_eq!((r.names.len(), r.partial), (MAX_NAMES, None), "exactly 500 is complete");
        let r = scan(&f, xmp_fonts, 100).unwrap();
        assert!(r.partial.unwrap().contains("first"), "size cap reported");
        std::fs::write(&f, xmp(&["A-Regular"])).unwrap();
        assert!(scan(&f, xmp_fonts, MAX_BYTES).unwrap().partial.is_none(), "a small file is not partial");
        std::fs::write(&f, b"").unwrap();
        assert!(scan(&f, xmp_fonts, MAX_BYTES).unwrap().names.is_empty(), "zero-byte file");
        assert!(scan(&dir, xmp_fonts, MAX_BYTES).is_err(), "a folder is an error, not a panic");
        std::fs::remove_dir_all(&dir).ok();
    }
}
