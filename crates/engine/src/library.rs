//! Library queries: families, detail, collections, tags, licenses, project sets.

use crate::activation::{parse_source_opt, set_keys, version_key};
use crate::scan::{family_key, numbered};
use crate::search::{self, Term};
use crate::{
    not_found, store, ActivationResult, ActiveState, Axis, Builtin, Category, Collection, Engine, EngineError, ExportResult, Face,
    Family, FamilyDetail, FamilyFilter, FamilyPage, FamilyQuery, FileRow, FileStatus, FontDetails, GlyphMap, LanguageCount, LibraryCounts, License,
    LicenseInfo, ProjectSet, Result, SortBy, TagCount,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use typeshelf_core::glyphs::{covers, glyph_map, Coverage};
use typeshelf_core::languages::{language, LANGUAGES};
use typeshelf_core::{font_details, Metrics};
use typeshelf_protocol::Source;

/// One face of one file copy.
struct Row {
    hash: String,
    idx: i64,
    family_key: String,
    ps_name: String,
    family: String,
    style: String,
    full_name: String,
    version: String,
    foundry: String,
    designer: String,
    weight: f32,
    italic: bool,
    is_variable: bool,
    glyph_count: u32,
    axes_json: String,
    features_json: String,
    path: String,
    folder_id: i64,
    added_at: String,
    source: Option<Source>,
    active_path: Option<String>,
    /// The copy lives in an OS font folder, so the OS has it on regardless of the ledger.
    system: bool,
    category: Option<Category>,
    temporary_until: Option<String>,
}

impl Row {
    fn active(&self) -> bool {
        self.source.is_some() || self.system
    }
}

fn load_rows(conn: &Connection, key: Option<&str>) -> rusqlite::Result<Vec<Row>> {
    let mut stmt = conn.prepare_cached(
        "SELECT f.hash, f.idx, f.family_key, f.ps_name, f.family, f.style, COALESCE(d.display_name, f.full_name),
                f.version, f.foundry, f.designer, f.weight, f.italic, f.is_variable, f.glyph_count, f.axes_json,
                f.features_json, fo.path, fi.rel_path, fi.folder_id, fi.added_at, a.source, a.path,
                fo.managed IS 'system', f.category,
                (SELECT json_extract(value, '$.\"' || f.hash || '\"') FROM settings WHERE key = 'temporary')
         FROM faces f
         JOIN files fi ON fi.hash = f.hash
         JOIN folders fo ON fo.id = fi.folder_id
         LEFT JOIN activations a ON a.hash = f.hash
         LEFT JOIN display_overrides d ON d.hash = f.hash
         WHERE ?1 IS NULL OR f.family_key = ?1
         ORDER BY f.family_key, f.weight, f.italic, f.style, f.hash, f.idx, fi.id",
    )?;
    let rows = stmt.query_map([key], |r| {
        Ok(Row {
            hash: r.get(0)?,
            idx: r.get(1)?,
            family_key: r.get(2)?,
            ps_name: r.get(3)?,
            family: r.get(4)?,
            style: r.get(5)?,
            full_name: r.get(6)?,
            version: r.get(7)?,
            foundry: r.get(8)?,
            designer: r.get(9)?,
            weight: r.get(10)?,
            italic: r.get(11)?,
            is_variable: r.get(12)?,
            glyph_count: r.get(13)?,
            axes_json: r.get(14)?,
            features_json: r.get(15)?,
            path: Path::new(&r.get::<_, String>(16)?).join(r.get::<_, String>(17)?).to_string_lossy().into_owned(),
            folder_id: r.get(18)?,
            added_at: r.get(19)?,
            source: parse_source_opt(r.get(20)?),
            active_path: r.get(21)?,
            system: r.get(22)?,
            category: r.get::<_, Option<String>>(23)?.as_deref().and_then(Category::parse),
            temporary_until: r.get(24)?,
        })
    })?;
    rows.collect()
}

/// Rank of the face that stands for its family (lowest wins): upright, weight nearest 400.
fn rep_rank(weight: f32, italic: bool) -> (bool, u32) {
    (italic, (weight - 400.0).abs() as u32)
}

/// The face that stands for its family (category, glyph search, details).
fn rep<'a>(rows: &[&'a Row]) -> Option<&'a Row> {
    rows.iter().copied().min_by_key(|r| rep_rank(r.weight, r.italic))
}

fn pins(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare_cached("SELECT family_key FROM pins")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    rows.collect()
}

/// Whether face (`hash`, `idx`) maps every one of `chars`.
fn face_covers(conn: &Connection, hash: &str, idx: i64, chars: &[u32]) -> rusqlite::Result<bool> {
    let json: String = conn
        .prepare_cached("SELECT coverage_json FROM faces WHERE hash = ?1 AND idx = ?2")?
        .query_row(params![hash, idx], |r| r.get(0))?;
    let cov: Coverage = serde_json::from_str(&json).unwrap_or_default();
    Ok(chars.iter().all(|c| covers(&cov, *c)))
}

/// Tags per family key, sorted.
// ponytail: reads every tag row; fine for thousands of tags.
fn tags_of(conn: &Connection) -> rusqlite::Result<HashMap<String, Vec<String>>> {
    let mut stmt = conn.prepare_cached("SELECT family_key, tag FROM family_tags ORDER BY tag COLLATE NOCASE")?;
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (k, t) = row?;
        out.entry(k).or_default().push(t);
    }
    Ok(out)
}

fn licenses(conn: &Connection) -> rusqlite::Result<HashMap<String, License>> {
    let mut stmt = conn.prepare_cached("SELECT family_key, label FROM licenses")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, License::parse(&r.get::<_, String>(1)?))))?;
    rows.collect()
}

fn face(row: &Row, path: &str) -> Face {
    Face {
        hash: row.hash.clone(),
        ps_name: row.ps_name.clone(),
        family: row.family.clone(),
        style: row.style.clone(),
        full_name: row.full_name.clone(),
        version: row.version.clone(),
        path: path.into(),
        active: row.active(),
        source: row.source,
        weight: row.weight,
        italic: row.italic,
        is_variable: row.is_variable,
        system: row.system,
        temporary_until: row.temporary_until.clone(),
    }
}

/// Faces, one per (hash, face index); `path` is the registered copy when active.
fn unique_faces(rows: &[&Row]) -> Vec<Face> {
    let mut seen: BTreeMap<(&str, i64), usize> = BTreeMap::new();
    let mut faces: Vec<Face> = Vec::new();
    for r in rows {
        match seen.get(&(r.hash.as_str(), r.idx)) {
            Some(&i) => {
                faces[i].active |= r.active();
                faces[i].system |= r.system;
                if r.active_path.as_deref() == Some(r.path.as_str()) {
                    faces[i].path = r.path.clone();
                }
            }
            None => {
                seen.insert((&r.hash, r.idx), faces.len());
                faces.push(face(r, &r.path));
            }
        }
    }
    faces
}

fn build_family(key: &str, rows: &[&Row], license: License) -> Family {
    let faces = unique_faces(rows);
    let mut hashes_by_ps: HashMap<String, HashSet<&str>> = HashMap::new();
    let mut paths_by_hash: HashMap<&str, HashSet<&str>> = HashMap::new();
    for r in rows {
        hashes_by_ps.entry(r.ps_name.to_lowercase()).or_default().insert(&r.hash);
        paths_by_hash.entry(&r.hash).or_default().insert(&r.path);
    }
    let active_ps: HashSet<String> = faces.iter().filter(|f| f.active).map(|f| f.ps_name.to_lowercase()).collect();
    let active_state = match active_ps.len() {
        0 => ActiveState::None,
        n if n == hashes_by_ps.len() => ActiveState::All,
        _ => ActiveState::Some,
    };
    Family {
        key: key.into(),
        family: rows[0].family.clone(),
        foundry: rows[0].foundry.clone(),
        styles: hashes_by_ps.len() as u32,
        is_variable: faces.iter().any(|f| f.is_variable),
        license,
        active_state,
        admin_set: faces.iter().any(|f| f.source == Some(Source::Admin)),
        has_duplicates: hashes_by_ps.values().any(|h| h.len() > 1) || paths_by_hash.values().any(|p| p.len() > 1),
        system: rows.iter().all(|r| r.system),
        category: rep(rows).and_then(|r| r.category),
        tags: Vec::new(), // filled by the caller, which has the tag table loaded
        pinned: false,    // same
        faces,
    }
}

fn matches_search(f: &Family, needle: &str) -> bool {
    f.family.to_lowercase().contains(needle)
        || f.foundry.to_lowercase().contains(needle)
        || f.faces.iter().any(|x| x.ps_name.to_lowercase().contains(needle) || x.full_name.to_lowercase().contains(needle))
}

/// One SuperSearch token; `Chars` is checked by the caller against the coverage.
// ponytail: weight compares each face's default weight, not a variable font's wght range.
fn term_matches(t: &Term, f: &Family, rows: &[&Row]) -> bool {
    match t {
        Term::Tag(tag) => f.tags.iter().any(|x| x.to_lowercase() == *tag),
        Term::Foundry(v) => f.foundry.to_lowercase().contains(v.as_str()),
        Term::Designer(v) => rows.iter().any(|r| r.designer.to_lowercase().contains(v.as_str())),
        Term::Category(c) => f.category == Some(*c),
        Term::License(l) => f.license == *l,
        Term::Weight(lo, hi) => f.faces.iter().any(|x| x.weight >= *lo && x.weight <= *hi),
        Term::WeightAbove(w) => f.faces.iter().any(|x| x.weight > *w),
        Term::WeightBelow(w) => f.faces.iter().any(|x| x.weight < *w),
        Term::Italic => f.faces.iter().any(|x| x.italic),
        Term::Variable => f.is_variable,
        Term::Active => f.active_state != ActiveState::None,
        Term::Inactive => f.active_state == ActiveState::None,
        Term::Pinned => f.pinned,
        Term::System => rows.iter().any(|r| r.system),
        Term::Chars(_) | Term::Lang(_) => true,
    }
}

/// The shape of a family's representative face, for [`distance`].
struct Shape {
    weight: f32,
    italic: bool,
    metrics: Metrics,
}

/// Weighted L1 distance between two families. Every term is about 0-1 over the usual range; a
/// different category counts double so a sans never ranks behind a mono that happens to be close.
// ponytail: hand-tuned weights over font metadata; add glyph-outline features (stroke contrast,
// terminals) if the matches feel off.
fn distance(a: &Shape, b: &Shape) -> f32 {
    let (ma, mb) = (&a.metrics, &b.metrics);
    let flag = |differ: bool, w: f32| if differ { w } else { 0.0 };
    let mut d = match (ma.category, mb.category) {
        (Some(x), Some(y)) => flag(x != y, 2.0),
        (None, None) => 0.0,
        _ => 1.0,
    };
    d += (a.weight - b.weight).abs() / 300.0 + flag(a.italic != b.italic, 0.5);
    d += (ma.width_class as f32 - mb.width_class as f32).abs() / 4.0;
    for (x, y, range) in [(ma.x_height, mb.x_height, 0.1), (ma.cap_height, mb.cap_height, 0.1), (ma.avg_width, mb.avg_width, 0.15)] {
        if x > 0.0 && y > 0.0 {
            d += (x - y).abs() / range;
        }
    }
    // PANOSE Latin text: serif style, proportion, contrast.
    let (pa, pb) = (&ma.panose, &mb.panose);
    if pa[0] == 2 && pb[0] == 2 {
        d += flag(pa[1] != pb[1], 0.5) + flag(pa[3] != pb[3], 0.5) + (pa[4] as f32 - pb[4] as f32).abs() / 8.0;
    }
    d
}

/// Family keys of a collection and every collection below it.
fn collection_keys(conn: &Connection, id: i64) -> Result<Vec<String>> {
    if conn.query_row("SELECT 1 FROM collections WHERE id = ?1", [id], |_| Ok(())).optional()?.is_none() {
        return Err(not_found("collection"));
    }
    let mut stmt = conn.prepare(
        "WITH RECURSIVE c(id) AS (SELECT ?1 UNION SELECT k.id FROM collections k JOIN c ON k.parent_id = c.id)
         SELECT DISTINCT family_key FROM collection_items WHERE collection_id IN (SELECT id FROM c)",
    )?;
    let keys = stmt.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(keys)
}

pub(crate) fn hidden_dirs(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached("SELECT path FROM hidden_folders")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    rows.collect()
}

fn keys_where(conn: &Connection, sql: &str, arg: impl rusqlite::ToSql) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([arg], |r| r.get(0))?;
    rows.collect()
}

pub(crate) fn license_info(conn: &Connection, key: &str) -> rusqlite::Result<LicenseInfo> {
    Ok(conn
        .query_row("SELECT label, file_path, note FROM licenses WHERE family_key = ?1", [key], |r| {
            Ok(LicenseInfo { label: License::parse(&r.get::<_, String>(0)?), file: r.get(1)?, note: r.get(2)? })
        })
        .optional()?
        .unwrap_or(LicenseInfo { label: License::Unknown, file: None, note: String::new() }))
}

pub(crate) fn set_license_row(conn: &Connection, key: &str, label: License, file: Option<&str>, note: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO licenses (family_key, label, file_path, note) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (family_key) DO UPDATE SET label = excluded.label, file_path = excluded.file_path, note = excluded.note",
        params![key, label.as_str(), file, note],
    )?;
    Ok(())
}

fn file_status(r: &Row, rows: &[&Row], mixed: bool) -> FileStatus {
    let ps = r.ps_name.to_lowercase();
    let same_ps: Vec<&&Row> = rows.iter().filter(|o| o.ps_name.to_lowercase() == ps).collect();
    let newest = same_ps.iter().map(|o| version_key(&o.version)).max().unwrap_or_default();
    if r.system {
        FileStatus::System
    } else if r.active_path.as_deref() == Some(r.path.as_str()) {
        FileStatus::Active
    } else if r.active() {
        FileStatus::Alternate
    } else if version_key(&r.version) < newest {
        FileStatus::OlderVersion
    } else if mixed {
        FileStatus::Grouped
    } else if same_ps.iter().any(|o| o.hash != r.hash) {
        FileStatus::Alternate
    } else {
        FileStatus::Inactive
    }
}

impl Engine {
    pub fn list_families(&self, q: &FamilyQuery) -> Result<Vec<Family>> {
        let db = self.db();
        let rows = load_rows(&db, None)?;
        let licenses = licenses(&db)?;
        let tags_by_key = tags_of(&db)?;
        let collection = match q.collection_id {
            Some(id) => Some(keys_where(&db, "SELECT family_key FROM collection_items WHERE collection_id = ?1", id)?),
            None => None,
        };
        let tagged = match &q.tag {
            Some(t) => Some(keys_where(&db, "SELECT family_key FROM family_tags WHERE tag = ?1", t)?),
            None => None,
        };
        let search = search::parse(q.search.as_deref().unwrap_or(""));
        let wants_system = search.terms.contains(&Term::System);
        let mut chars: Vec<u32> = search
            .terms
            .iter()
            .filter_map(|t| if let Term::Chars(c) = t { Some(c.as_str()) } else { None })
            .chain(q.chars.as_deref())
            .flat_map(str::chars)
            .filter(|c| !c.is_whitespace())
            .map(u32::from)
            .collect();
        // A language is its required characters, so it rides the glyph search (representative face).
        let wanted = search.terms.iter().filter_map(|t| if let Term::Lang(l) = t { Some(l.as_str()) } else { None }).chain(q.lang.as_deref());
        for id in wanted {
            match language(id) {
                Some(l) => chars.extend(l.chars()),
                None => return Ok(Vec::new()),
            }
        }
        let pins = pins(&db)?;
        let hidden = hidden_dirs(&db)?;
        let in_hidden = |p: &str| hidden.iter().any(|h| Path::new(p).starts_with(h));
        let show_hidden = q.include_hidden || q.keys.is_some() || q.path.as_deref().is_some_and(in_hidden);

        let mut groups: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
        for r in &rows {
            groups.entry(&r.family_key).or_default().push(r);
        }
        let mut out: Vec<(Family, &str)> = Vec::new();
        for (key, rows) in groups {
            if collection.as_ref().is_some_and(|c| !c.contains(key))
                || tagged.as_ref().is_some_and(|t| !t.contains(key))
                || q.keys.as_ref().is_some_and(|k| !k.iter().any(|k| k == key))
                || q.folder_id.is_some_and(|id| !rows.iter().any(|r| r.folder_id == id))
                || q.path.as_deref().is_some_and(|p| !rows.iter().any(|r| Path::new(&r.path).starts_with(p)))
                || (!show_hidden && !hidden.is_empty() && rows.iter().all(|r| in_hidden(&r.path)))
            {
                continue;
            }
            let mut fam = build_family(key, &rows, licenses.get(key).copied().unwrap_or(License::Unknown));
            fam.tags = tags_by_key.get(key).cloned().unwrap_or_default();
            fam.pinned = pins.contains(key);
            // System-only families stay out of the library lists unless asked for (or collected/tagged/picked).
            if fam.system && q.filter != FamilyFilter::System && collection.is_none() && tagged.is_none() && q.keys.is_none() && !wants_system {
                continue;
            }
            let keep = match q.filter {
                FamilyFilter::System => rows.iter().any(|r| r.system),
                FamilyFilter::All => true,
                FamilyFilter::Active => fam.active_state != ActiveState::None,
                FamilyFilter::Inactive => fam.active_state == ActiveState::None,
                FamilyFilter::Admin => fam.admin_set,
                FamilyFilter::UnknownLicense => fam.license == License::Unknown,
                FamilyFilter::Duplicates => fam.has_duplicates,
            };
            if !keep
                || q.license.is_some_and(|l| l != fam.license)
                || q.category.is_some_and(|c| fam.category != Some(c))
                || search.text.as_deref().is_some_and(|n| !matches_search(&fam, n))
                || !search.terms.iter().all(|t| term_matches(t, &fam, &rows))
            {
                continue;
            }
            if !chars.is_empty() {
                // ponytail: one indexed lookup + JSON parse per family that got this far; cache
                // parsed coverage if glyph search over huge libraries feels slow.
                let Some(r) = rep(&rows) else { continue };
                if !face_covers(&db, &r.hash, r.idx, &chars)? {
                    continue;
                }
            }
            let added = rows.iter().map(|r| r.added_at.as_str()).max().unwrap_or("");
            out.push((fam, added));
        }
        let name = |f: &Family| f.family.to_lowercase();
        match q.sort {
            SortBy::Name => out.sort_by_key(|(f, _)| name(f)),
            SortBy::NameDesc => out.sort_by_key(|(f, _)| std::cmp::Reverse(name(f))),
            SortBy::Date => out.sort_by(|a, b| b.1.cmp(a.1).then_with(|| name(&a.0).cmp(&name(&b.0)))),
            SortBy::Foundry => out.sort_by_key(|(f, _)| (f.foundry.to_lowercase(), name(f))),
            SortBy::Random => {
                let seed = q.seed.unwrap_or(0.0).to_bits();
                out.sort_by_cached_key(|(f, _)| {
                    let mut h = DefaultHasher::new();
                    (seed, &f.key).hash(&mut h);
                    h.finish()
                });
            }
        }
        out.sort_by_key(|(f, _)| !f.pinned); // stable: pinned first, each part keeps its order
        Ok(out.into_iter().map(|(f, _)| f).collect())
    }

    /// One page of `list_families` (at most 200 families).
    // ponytail: builds the whole filtered list and slices it; fine for tens of thousands of faces,
    // push LIMIT/OFFSET into SQL if libraries get far bigger.
    pub fn family_page(&self, q: &FamilyQuery, offset: usize, limit: usize) -> Result<FamilyPage> {
        let all = self.list_families(q)?;
        let total = all.len();
        Ok(FamilyPage { items: all.into_iter().skip(offset).take(limit.min(200)).collect(), total })
    }

    pub fn library_counts(&self) -> Result<LibraryCounts> {
        let library = self.list_families(&FamilyQuery::default())?;
        let system = self.list_families(&FamilyQuery { filter: FamilyFilter::System, ..Default::default() })?.len();
        let mut licenses = BTreeMap::new();
        for f in &library {
            *licenses.entry(f.license.as_str().to_string()).or_insert(0) += 1;
        }
        Ok(LibraryCounts {
            library: library.len(),
            active: library.iter().filter(|f| f.active_state != ActiveState::None).count(),
            system,
            licenses,
        })
    }

    /// Every language with the number of library families whose representative face covers it.
    // ponytail: reads each family's representative coverage once (a JSON parse per family), no
    // cache; fine for thousands of families, store per-language flags per face if it gets slow.
    pub fn language_counts(&self) -> Result<Vec<LanguageCount>> {
        let keys: HashSet<String> = self.list_families(&FamilyQuery::default())?.into_iter().map(|f| f.key).collect();
        // The guard is held only to copy each family's representative coverage text out.
        let json: Vec<String> = {
            let db = self.db();
            let mut stmt = db.prepare_cached(
                "SELECT family_key, weight, italic, coverage_json FROM faces f WHERE EXISTS (SELECT 1 FROM files WHERE hash = f.hash)
                 ORDER BY weight, italic, style, hash, idx",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f32>(1)?, r.get::<_, bool>(2)?, r.get::<_, String>(3)?)))?;
            let mut best: HashMap<String, (f32, bool, String)> = HashMap::new();
            for row in rows {
                let (k, weight, italic, json) = row?;
                if keys.contains(&k) && best.get(&k).is_none_or(|b| rep_rank(weight, italic) < rep_rank(b.0, b.1)) {
                    best.insert(k, (weight, italic, json));
                }
            }
            best.into_values().map(|b| b.2).collect()
        };
        let mut counts = vec![0; LANGUAGES.len()];
        for j in &json {
            let cov: Coverage = serde_json::from_str(j).unwrap_or_default();
            for (n, l) in counts.iter_mut().zip(LANGUAGES) {
                *n += usize::from(l.covered_by(&cov));
            }
        }
        Ok(LANGUAGES.iter().zip(counts).map(|(l, families)| LanguageCount { id: l.id.into(), name: l.name.into(), families }).collect())
    }

    /// Families that look most like `key`, nearest first (system fonts included). See [`distance`].
    pub fn similar_families(&self, key: &str, limit: usize) -> Result<Vec<Family>> {
        let shapes: HashMap<String, Shape> = {
            let db = self.db();
            let mut stmt = db.prepare(
                "SELECT family_key, weight, italic, metrics_json FROM faces f WHERE EXISTS (SELECT 1 FROM files WHERE hash = f.hash)",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f32>(1)?, r.get::<_, bool>(2)?, r.get::<_, String>(3)?)))?;
            let mut best: HashMap<String, (f32, bool, String)> = HashMap::new();
            for row in rows {
                let (k, weight, italic, json) = row?;
                if best.get(&k).is_none_or(|b| rep_rank(weight, italic) < rep_rank(b.0, b.1)) {
                    best.insert(k, (weight, italic, json));
                }
            }
            best.into_iter()
                .map(|(k, (weight, italic, json))| (k, Shape { weight, italic, metrics: serde_json::from_str(&json).unwrap_or_default() }))
                .collect()
        };
        let me = shapes.get(key).ok_or_else(|| not_found("family"))?;
        let mut ranked: Vec<(f32, &String)> = shapes.iter().filter(|(k, _)| *k != key).map(|(k, s)| (distance(me, s), k)).collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        let keys: Vec<String> = ranked.into_iter().take(limit.min(50)).map(|(_, k)| k.clone()).collect();
        let mut families = self.list_families(&FamilyQuery { keys: Some(keys.clone()), ..Default::default() })?;
        families.sort_by_key(|f| keys.iter().position(|k| *k == f.key));
        Ok(families)
    }

    /// Faces by file hash (for previews of fonts found elsewhere, e.g. in a PDF).
    pub fn faces(&self, hashes: &[String]) -> Result<Vec<Face>> {
        let wanted: HashSet<&str> = hashes.iter().map(String::as_str).collect();
        let db = self.db();
        let rows = load_rows(&db, None)?;
        let rows: Vec<&Row> = rows.iter().filter(|r| wanted.contains(r.hash.as_str())).collect();
        Ok(unique_faces(&rows))
    }

    pub fn get_family(&self, key: &str) -> Result<FamilyDetail> {
        let db = self.db();
        let rows = load_rows(&db, Some(key))?;
        if rows.is_empty() {
            return Err(not_found("family"));
        }
        let rows: Vec<&Row> = rows.iter().collect();
        let info = license_info(&db, key)?;
        let mut family = build_family(key, &rows, info.label);
        family.tags = tags_of(&db)?.remove(key).unwrap_or_default();
        family.pinned = pins(&db)?.contains(key);
        let details = rep(&rows)
            .and_then(|r| {
                rows.iter()
                    .filter(|o| o.hash == r.hash)
                    .find_map(|o| std::fs::read(&o.path).ok())
                    .and_then(|bytes| font_details(&bytes, r.idx as u32))
            })
            .unwrap_or_else(|| FontDetails { glyph_count: rows[0].glyph_count, ..Default::default() });
        let axes: Vec<Axis> = rows
            .iter()
            .find(|r| r.is_variable)
            .and_then(|r| serde_json::from_str(&r.axes_json).ok())
            .unwrap_or_default();
        let features: BTreeSet<String> =
            rows.iter().flat_map(|r| serde_json::from_str::<Vec<String>>(&r.features_json).unwrap_or_default()).collect();
        let mixed = rows.iter().any(|r| r.is_variable) && rows.iter().any(|r| !r.is_variable);
        let mut seen = HashSet::new();
        let files = rows
            .iter()
            .filter(|r| seen.insert(r.path.as_str()))
            .map(|r| FileRow { path: r.path.clone(), version: r.version.clone(), status: file_status(r, &rows, mixed) })
            .collect();
        Ok(FamilyDetail {
            axes,
            features: features.into_iter().collect(),
            glyph_count: rows.iter().map(|r| r.glyph_count).max().unwrap_or(0),
            designer: rows.iter().map(|r| r.designer.as_str()).find(|d| !d.is_empty()).unwrap_or("").into(),
            files,
            license_info: info,
            details,
            family,
        })
    }

    /// The cmap of one face (`index` within a TTC), grouped by Unicode block. Read from the first
    /// readable copy of the file.
    pub fn face_glyphs(&self, hash: &str, index: u32) -> Result<GlyphMap> {
        let paths: Vec<PathBuf> = {
            let db = self.db();
            let mut stmt = db.prepare(
                "SELECT fo.path, fi.rel_path FROM files fi JOIN folders fo ON fo.id = fi.folder_id WHERE fi.hash = ?1 ORDER BY fi.id",
            )?;
            let rows = stmt.query_map([hash], |r| Ok(Path::new(&r.get::<_, String>(0)?).join(r.get::<_, String>(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        paths
            .iter()
            .find_map(|p| std::fs::read(p).ok())
            .and_then(|bytes| glyph_map(&bytes, index))
            .ok_or_else(|| not_found("font file"))
    }

    /// Pins or unpins families; pinned families sort first in every list.
    pub fn set_pinned(&self, keys: &[String], pinned: bool) -> Result<()> {
        {
            let mut db = self.db();
            let tx = db.transaction()?;
            for k in keys {
                if pinned {
                    tx.execute("INSERT OR IGNORE INTO pins (family_key) VALUES (?1)", [k])?;
                } else {
                    tx.execute("DELETE FROM pins WHERE family_key = ?1", [k])?;
                }
            }
            tx.commit()?;
        }
        self.library_changed("scan");
        Ok(())
    }

    /// Faces for the quick-activate popup, at most 20.
    pub fn quick_search(&self, query: &str) -> Result<Vec<Face>> {
        let needle = query.trim().to_lowercase();
        let db = self.db();
        let rows = load_rows(&db, None)?;
        let hits: Vec<&Row> = rows
            .iter()
            .filter(|r| {
                needle.is_empty()
                    || r.family.to_lowercase().contains(&needle)
                    || r.full_name.to_lowercase().contains(&needle)
                    || r.ps_name.to_lowercase().contains(&needle)
            })
            .collect();
        let mut faces = unique_faces(&hits);
        faces.sort_by_key(|f| (!f.family.to_lowercase().starts_with(&needle), f.family.to_lowercase(), f.weight as i32, f.italic));
        faces.truncate(20);
        Ok(faces)
    }

    pub fn list_collections(&self) -> Result<Vec<Collection>> {
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT c.id, c.name, c.parent_id, c.builtin, (SELECT COUNT(*) FROM collection_items WHERE collection_id = c.id),
                    (SELECT COUNT(*) FROM collection_items ci WHERE ci.collection_id = c.id
                       AND EXISTS (SELECT 1 FROM faces f JOIN activations a ON a.hash = f.hash WHERE f.family_key = ci.family_key))
             FROM collections c ORDER BY c.builtin IS NULL, c.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            let builtin: Option<String> = r.get(3)?;
            Ok(Collection {
                id: r.get(0)?,
                name: r.get(1)?,
                parent_id: r.get(2)?,
                count: r.get(4)?,
                active: r.get(5)?,
                builtin: builtin.map(|b| if b == "recent" { Builtin::Recent } else { Builtin::Favorites }),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn create_collection(&self, name: &str, parent_id: Option<i64>) -> Result<Vec<Collection>> {
        let name = non_empty(name)?;
        self.db().execute("INSERT INTO collections (name, parent_id) VALUES (?1, ?2)", params![name, parent_id])?;
        self.list_collections()
    }

    pub fn rename_collection(&self, id: i64, name: &str) -> Result<Vec<Collection>> {
        let name = non_empty(name)?;
        self.editable_collection(id)?;
        self.db().execute("UPDATE collections SET name = ?2 WHERE id = ?1", params![id, name])?;
        self.list_collections()
    }

    pub fn delete_collection(&self, id: i64) -> Result<Vec<Collection>> {
        self.editable_collection(id)?;
        self.db().execute("DELETE FROM collections WHERE id = ?1", [id])?;
        self.list_collections()
    }

    fn editable_collection(&self, id: i64) -> Result<()> {
        let builtin: Option<Option<String>> =
            self.db().query_row("SELECT builtin FROM collections WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        match builtin {
            None => Err(not_found("collection")),
            Some(Some(_)) => Err(EngineError::new("builtin", "built-in collections cannot be renamed or deleted")),
            Some(None) => Ok(()),
        }
    }

    pub fn add_to_collection(&self, id: i64, family_keys: &[String]) -> Result<Vec<Collection>> {
        {
            let mut db = self.db();
            let tx = db.transaction()?;
            let now = store::now();
            for k in family_keys {
                tx.execute(
                    "INSERT OR IGNORE INTO collection_items (collection_id, family_key, added_at) VALUES (?1, ?2, ?3)",
                    params![id, k, now],
                )?;
            }
            tx.commit()?;
        }
        self.list_collections()
    }

    pub fn remove_from_collection(&self, id: i64, family_keys: &[String]) -> Result<Vec<Collection>> {
        {
            let db = self.db();
            for k in family_keys {
                db.execute("DELETE FROM collection_items WHERE collection_id = ?1 AND family_key = ?2", params![id, k])?;
            }
        }
        self.list_collections()
    }

    /// Turns every family in the collection and its sub-collections on or off. Turning on leaves a
    /// font alone when another version of it is already on.
    pub fn set_collection_active(&self, id: i64, active: bool) -> Result<Vec<ActivationResult>> {
        let hashes = {
            let db = self.db();
            let keys = collection_keys(&db, id)?;
            let mut stmt = db.prepare(
                "SELECT DISTINCT f.hash FROM faces f WHERE f.family_key = ?1
                   AND EXISTS (SELECT 1 FROM files WHERE hash = f.hash)
                   AND (?2 = 0 OR NOT EXISTS (SELECT 1 FROM activations a JOIN faces g ON g.hash = a.hash
                                              WHERE g.ps_name = f.ps_name COLLATE NOCASE AND a.hash != f.hash))",
            )?;
            let mut hashes = Vec::new();
            for k in &keys {
                hashes.extend(stmt.query_map(params![k, active], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?);
            }
            hashes
        };
        self.set_active(&hashes, active)
    }

    /// Zips one copy of every font file in the collection (and below), leaving out OS font folders.
    pub fn export_collection(&self, id: i64, dest: &str) -> Result<ExportResult> {
        let files: Vec<PathBuf> = {
            let db = self.db();
            let keys = collection_keys(&db, id)?;
            let mut stmt = db.prepare(
                "SELECT fi.hash, fo.path, fi.rel_path FROM files fi JOIN folders fo ON fo.id = fi.folder_id
                 WHERE fi.hash IN (SELECT hash FROM faces WHERE family_key = ?1) AND fo.managed IS NOT 'system'
                 ORDER BY fi.id",
            )?;
            let mut seen = HashSet::new();
            let mut files = Vec::new();
            for k in &keys {
                let rows = stmt.query_map([k], |r| Ok((r.get::<_, String>(0)?, Path::new(&r.get::<_, String>(1)?).join(r.get::<_, String>(2)?))))?;
                for row in rows {
                    let (hash, path) = row?;
                    if !seen.contains(&hash) && path.is_file() {
                        seen.insert(hash);
                        files.push(path);
                    }
                }
            }
            files
        };
        let zip_err = |e: zip::result::ZipError| EngineError::new("io", e.to_string());
        let mut zip = zip::ZipWriter::new(std::fs::File::create(dest)?);
        let mut names = HashSet::new();
        for path in &files {
            let file_name = Path::new(path.file_name().unwrap_or_default());
            let name = (1..)
                .map(|n| numbered(file_name, n).to_string_lossy().into_owned())
                .find(|n| names.insert(n.to_lowercase()))
                .unwrap_or_default();
            zip.start_file(name, zip::write::SimpleFileOptions::default()).map_err(zip_err)?;
            std::io::copy(&mut std::fs::File::open(path)?, &mut zip)?;
        }
        zip.finish().map_err(zip_err)?;
        Ok(ExportResult { files: files.len() as u32, path: dest.into() })
    }

    /// FontBase "override family": these files are listed under `name` (None restores the parsed
    /// family). Kept in `family_overrides`, so rescans keep it; the font files are never touched.
    /// A family that moves as a whole takes its collections, tags, license and project sets along.
    pub fn set_family_name(&self, hashes: &[String], name: Option<&str>) -> Result<()> {
        let name = name.map(non_empty).transpose()?;
        {
            let mut db = self.db();
            let tx = db.transaction()?;
            let mut moves = HashSet::new();
            for h in hashes {
                match name {
                    Some(n) => tx.execute("INSERT OR REPLACE INTO family_overrides (hash, family) VALUES (?1, ?2)", params![h, n])?,
                    None => tx.execute("DELETE FROM family_overrides WHERE hash = ?1", [h])?,
                };
                let faces: Vec<(i64, String, String, String)> = tx
                    .prepare_cached("SELECT idx, family_key, COALESCE(real_family, family), foundry FROM faces WHERE hash = ?1")?
                    .query_map([h], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                    .collect::<rusqlite::Result<_>>()?;
                for (idx, old_key, real, foundry) in faces {
                    let family = name.unwrap_or(&real);
                    let key = family_key(family, &foundry);
                    tx.execute(
                        "UPDATE faces SET real_family = COALESCE(real_family, family), family = ?3, family_key = ?4
                         WHERE hash = ?1 AND idx = ?2",
                        params![h, idx, family, key],
                    )?;
                    moves.insert((old_key, key));
                }
            }
            for (old, new) in moves.into_iter().filter(|(o, n)| o != n) {
                let left: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM faces WHERE family_key = ?1)", [&old], |r| r.get(0))?;
                if !left {
                    for table in ["collection_items", "family_tags", "licenses", "project_set_items", "pins", "usage_log"] {
                        tx.execute(&format!("UPDATE OR IGNORE {table} SET family_key = ?2 WHERE family_key = ?1"), params![old, new])?;
                    }
                }
            }
            tx.commit()?;
        }
        self.library_changed("scan");
        Ok(())
    }

    pub fn list_tags(&self) -> Result<Vec<TagCount>> {
        let db = self.db();
        let mut stmt = db.prepare("SELECT tag, COUNT(*) FROM family_tags GROUP BY tag ORDER BY tag COLLATE NOCASE")?;
        let rows = stmt.query_map([], |r| Ok(TagCount { name: r.get(0)?, count: r.get(1)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_tags(&self, family_key: &str, tags: &[String]) -> Result<Vec<String>> {
        let tags: BTreeSet<String> = tags.iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute("DELETE FROM family_tags WHERE family_key = ?1", [family_key])?;
        for t in &tags {
            tx.execute("INSERT INTO family_tags (family_key, tag) VALUES (?1, ?2)", params![family_key, t])?;
        }
        tx.commit()?;
        Ok(tags.into_iter().collect())
    }

    pub fn set_license(&self, family_key: &str, label: License, file: Option<&str>, note: Option<&str>) -> Result<LicenseInfo> {
        let db = self.db();
        // `None` keeps what is there: the UI edits the label, the file and the note one at a time.
        let cur = license_info(&db, family_key)?;
        set_license_row(&db, family_key, label, file.or(cur.file.as_deref()), note.unwrap_or(&cur.note))?;
        Ok(license_info(&db, family_key)?)
    }

    pub fn list_project_sets(&self) -> Result<Vec<ProjectSet>> {
        let db = self.db();
        let mut stmt = db.prepare("SELECT id, name, note, is_on, last_used FROM project_sets ORDER BY name COLLATE NOCASE")?;
        let sets: Vec<ProjectSet> = stmt
            .query_map([], |r| {
                Ok(ProjectSet {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    note: r.get(2)?,
                    family_keys: Vec::new(),
                    on: r.get(3)?,
                    last_used: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        sets.into_iter()
            .map(|mut s| {
                s.family_keys = set_keys(&db, s.id)?;
                Ok(s)
            })
            .collect()
    }

    pub fn save_project_set(&self, id: Option<i64>, name: &str, note: &str, family_keys: &[String]) -> Result<Vec<ProjectSet>> {
        let name = non_empty(name)?;
        {
            let mut db = self.db();
            let tx = db.transaction()?;
            let id = match id {
                Some(id) => {
                    if tx.execute("UPDATE project_sets SET name = ?2, note = ?3 WHERE id = ?1", params![id, name, note])? == 0 {
                        return Err(not_found("project set"));
                    }
                    tx.execute("DELETE FROM project_set_items WHERE set_id = ?1", [id])?;
                    id
                }
                None => {
                    tx.execute("INSERT INTO project_sets (name, note) VALUES (?1, ?2)", params![name, note])?;
                    tx.last_insert_rowid()
                }
            };
            for k in family_keys {
                tx.execute("INSERT OR IGNORE INTO project_set_items (set_id, family_key) VALUES (?1, ?2)", params![id, k])?;
            }
            tx.commit()?;
        }
        self.list_project_sets()
    }

    /// Deleting a set that is on turns it off first.
    pub fn delete_project_set(&self, id: i64) -> Result<Vec<ProjectSet>> {
        let on: bool = self
            .db()
            .query_row("SELECT is_on FROM project_sets WHERE id = ?1", [id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| not_found("project set"))?;
        if on {
            self.toggle_project_set(id, false)?;
        }
        self.db().execute("DELETE FROM project_sets WHERE id = ?1", [id])?;
        self.list_project_sets()
    }
}

fn non_empty(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(EngineError::new("invalid", "name is empty"));
    }
    Ok(name)
}
