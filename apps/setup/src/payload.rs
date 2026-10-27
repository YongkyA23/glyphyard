//! The zip built by scripts/build-windows.ps1: `typeshelf.exe` + `plugins/**`.

use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

static PAYLOAD: &[u8] = include_bytes!(env!("TYPESHELF_PAYLOAD"));

fn archive() -> io::Result<ZipArchive<Cursor<&'static [u8]>>> {
    ZipArchive::new(Cursor::new(PAYLOAD)).map_err(io::Error::other)
}

/// Uncompressed size in bytes.
pub fn size() -> u64 {
    // Not ZipArchive::decompressed_size: it gives up on entries with data descriptors, which tar writes.
    let Ok(mut zip) = archive() else { return 0 };
    (0..zip.len()).filter_map(|i| zip.by_index_raw(i).ok().map(|f| f.size())).sum()
}

/// Extracts every file into `dir`, calling `progress(relative path, bytes done, bytes total)` per file.
pub fn extract(dir: &Path, mut progress: impl FnMut(&Path, u64, u64)) -> io::Result<()> {
    let mut zip = archive()?;
    let total = size().max(1);
    let mut done = 0;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(io::Error::other)?;
        let rel: PathBuf = file
            .enclosed_name()
            .ok_or_else(|| io::Error::other(format!("unsafe path in payload entry {i}")))?;
        let out = dir.join(&rel);
        if file.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        io::copy(&mut file, &mut fs::File::create(&out)?)?;
        done += file.size();
        progress(&rel, done, total);
    }
    Ok(())
}
