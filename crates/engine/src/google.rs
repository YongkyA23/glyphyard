//! Google Fonts: catalog from Google's public metadata, family files from github.com/google/fonts.
//! Network code is the three small functions at the top; everything else is testable offline.

use crate::library::set_license_row;
use crate::{EngineError, Engine, GoogleFamily, GoogleInstall, GooglePage, License, Result};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use typeshelf_core::organize::sanitize;
use typeshelf_protocol::Source;

const METADATA_URL: &str = "https://fonts.google.com/metadata/fonts";
const CONTENTS_URL: &str = "https://api.github.com/repos/google/fonts/contents";

fn http_get(url: &str) -> Result<Vec<u8>> {
    let mut resp = ureq::get(url)
        .header("User-Agent", concat!("typeshelf/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(404) => crate::not_found(url),
            e => EngineError::new("offline", e.to_string()),
        })?;
    resp.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|e| EngineError::new("offline", e.to_string()))
}

fn fetch_catalog() -> Result<String> {
    Ok(String::from_utf8_lossy(&http_get(METADATA_URL)?).into_owned())
}

/// Downloads the family's .ttf/.otf files (top level of its directory, which holds the variable
/// fonts when there are any).
fn fetch_family(family: &str) -> Result<Vec<(String, Vec<u8>)>> {
    let dir = repo_dir(family);
    for license in ["ofl", "apache", "ufl"] {
        let listing = match http_get(&format!("{CONTENTS_URL}/{license}/{dir}")) {
            Ok(b) => b,
            Err(e) if e.code == "not_found" => continue,
            Err(e) => return Err(e),
        };
        let mut files = Vec::new();
        for (name, url) in font_entries(&listing)? {
            files.push((name, http_get(&url)?));
        }
        return Ok(files);
    }
    Err(crate::not_found(family))
}

/// "Noto Sans JP" → "notosansjp", the google/fonts directory name.
fn repo_dir(family: &str) -> String {
    family.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase()
}

#[derive(Deserialize)]
struct Entry {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    download_url: Option<String>,
}

fn font_entries(listing: &[u8]) -> Result<Vec<(String, String)>> {
    let entries: Vec<Entry> = serde_json::from_slice(listing)?;
    Ok(entries
        .into_iter()
        .filter(|e| e.kind == "file" && (e.name.ends_with(".ttf") || e.name.ends_with(".otf")))
        .filter_map(|e| Some((e.name, e.download_url?)))
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Meta {
    family: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    fonts: Map<String, Value>,
    #[serde(default)]
    axes: Vec<Value>,
}

/// How long the downloaded catalog is reused before asking Google again.
const CATALOG_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
pub const GOOGLE_PAGE_MAX: usize = 100;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    family_metadata_list: Vec<Meta>,
}

/// Parses the metadata JSON, which may start with the `)]}'` anti-XSSI prefix.
pub(crate) fn parse_catalog(text: &str) -> Result<Vec<Meta>> {
    let text = text.trim_start();
    let text = text.strip_prefix(")]}'").unwrap_or(text);
    Ok(serde_json::from_str::<Metadata>(text)?.family_metadata_list)
}

impl Engine {
    fn google_dir(&self) -> PathBuf {
        self.data_dir.join("Google Fonts")
    }

    fn catalog_cache(&self) -> PathBuf {
        self.data_dir.join("google-fonts.json")
    }

    /// Memory, then the disk copy if it is younger than a day, then the network (cached to disk),
    /// then any disk copy when offline. Sorted A–Z once, here.
    fn catalog(&self) -> Result<Arc<Vec<Meta>>> {
        let mut cached = self.google_catalog.lock().unwrap();
        if let Some(c) = cached.as_ref() {
            return Ok(c.clone());
        }
        let path = self.catalog_cache();
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|age| age < CATALOG_TTL));
        let text = match fresh.then(|| std::fs::read_to_string(&path).ok()).flatten() {
            Some(t) => t,
            None => match fetch_catalog() {
                Ok(t) => {
                    let _ = std::fs::write(&path, &t);
                    t
                }
                Err(e) => std::fs::read_to_string(&path).map_err(|_| e)?,
            },
        };
        let mut list = parse_catalog(&text)?;
        list.sort_by_cached_key(|m| m.family.to_lowercase());
        let list = Arc::new(list);
        *cached = Some(list.clone());
        Ok(list)
    }

    /// Google family names (lowercase, no spaces) known without touching the network.
    pub(crate) fn google_names(&self) -> HashSet<String> {
        let list = self.google_catalog.lock().unwrap().clone().or_else(|| {
            std::fs::read_to_string(self.catalog_cache()).ok().and_then(|t| parse_catalog(&t).ok()).map(Arc::new)
        });
        list.map(|l| l.iter().map(|m| m.family.to_lowercase().replace(' ', "")).collect()).unwrap_or_default()
    }

    /// One page of the catalog (A–Z), filtered by search text and category.
    pub fn google_catalog(&self, search: Option<&str>, category: Option<&str>, offset: usize, limit: usize) -> Result<GooglePage> {
        let needle = search.map(|s| s.trim().to_lowercase()).unwrap_or_default();
        let base = self.google_dir();
        let catalog = self.catalog()?;
        let matches: Vec<&Meta> = catalog
            .iter()
            .filter(|m| m.family.to_lowercase().contains(&needle))
            .filter(|m| category.is_none_or(|c| m.category.eq_ignore_ascii_case(c)))
            .collect();
        let items = matches
            .iter()
            .skip(offset)
            .take(limit.clamp(1, GOOGLE_PAGE_MAX))
            .map(|m| GoogleFamily {
                installed: base.join(sanitize(&m.family)).is_dir(),
                styles: m.fonts.len() as u32,
                variable: !m.axes.is_empty(),
                category: m.category.clone(),
                family: m.family.clone(),
            })
            .collect();
        Ok(GooglePage { items, total: matches.len() })
    }

    /// Downloads the family into the managed "Google Fonts" folder and labels it Licensed (OFL).
    pub fn google_install(&self, family: &str) -> Result<GoogleInstall> {
        let files = fetch_family(family)?;
        let (id, root) = self.managed_folder("google", "Google Fonts")?;
        let dir = root.join(sanitize(family));
        std::fs::create_dir_all(&dir)?;
        let mut hashes = Vec::new();
        for (name, bytes) in files {
            let name = sanitize(&name);
            if name.is_empty() || name.starts_with('.') {
                continue;
            }
            hashes.push(typeshelf_core::file_hash(&bytes));
            std::fs::write(dir.join(name), bytes)?;
        }
        self.scan_folder(id)?;
        {
            let db = self.db();
            let mut stmt = db.prepare("SELECT DISTINCT family_key FROM faces WHERE hash = ?1")?;
            let mut keys = HashSet::new();
            for h in &hashes {
                keys.extend(stmt.query_map([h], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?);
            }
            for k in keys {
                set_license_row(&db, &k, License::Licensed, None, "Google Fonts (open source)")?;
            }
        }
        self.library_changed("scan");
        Ok(GoogleInstall { family: family.into(), installed: true })
    }

    pub fn google_remove(&self, family: &str) -> Result<GoogleInstall> {
        let dir = self.google_dir().join(sanitize(family));
        if dir.is_dir() {
            {
                let db = self.db();
                let active: Vec<String> = self
                    .ledger(&db)?
                    .into_iter()
                    .filter(|a| std::path::Path::new(&a.path).starts_with(&dir))
                    .map(|a| a.hash)
                    .collect();
                self.deactivate_hashes(&db, &active, Source::User)?;
            }
            std::fs::remove_dir_all(&dir)?;
            let (id, _) = self.managed_folder("google", "Google Fonts")?;
            self.scan_folder(id)?;
            self.library_changed("scan");
        }
        Ok(GoogleInstall { family: family.into(), installed: false })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#")]}'
{"axisRegistry":[],"familyMetadataList":[
 {"family":"Roboto Flex","displayName":null,"category":"Sans Serif","size":1,"subsets":["latin"],
  "fonts":{"400":{"thickness":4,"slant":1,"width":6,"lineHeight":1.2}},
  "axes":[{"tag":"wght","min":100,"max":1000,"defaultValue":400}],"designers":["Font Bureau"]},
 {"family":"Lora","category":"Serif","fonts":{"400":{},"400i":{},"700":{},"700i":{}},"axes":[]}
]}"#;

    #[test]
    fn parses_metadata_with_prefix() {
        let list = parse_catalog(SAMPLE).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].family, "Roboto Flex");
        assert!(!list[0].axes.is_empty());
        assert_eq!(list[1].fonts.len(), 4);
        assert_eq!(parse_catalog(SAMPLE.trim_start_matches(")]}'")).unwrap().len(), 2);
    }

    #[test]
    fn picks_font_files_from_listing() {
        let listing = br#"[
          {"name":"OFL.txt","type":"file","download_url":"https://raw/OFL.txt"},
          {"name":"Lora[wght].ttf","type":"file","download_url":"https://raw/Lora%5Bwght%5D.ttf"},
          {"name":"static","type":"dir","download_url":null}]"#;
        let files = font_entries(listing).unwrap();
        assert_eq!(files, vec![("Lora[wght].ttf".to_string(), "https://raw/Lora%5Bwght%5D.ttf".to_string())]);
        assert_eq!(repo_dir("Noto Sans JP"), "notosansjp");
    }
}
