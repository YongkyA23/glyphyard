//! Library queries: families, detail, collections, tags, licenses, project sets.

use crate::activation::{parse_source_opt, set_keys, version_key};
use crate::{
    not_found, store, ActiveState, Axis, Builtin, Collection, Engine, EngineError, Face, Family, FamilyDetail,
    FamilyFilter, FamilyPage, FamilyQuery, FileRow, FileStatus, LibraryCounts, License, LicenseInfo, ProjectSet, Result, SortBy, TagCount,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
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
                fo.managed IS 'system'
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
        })
    })?;
    rows.collect()
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
        faces,
    }
}

fn matches_search(f: &Family, needle: &str) -> bool {
    f.family.to_lowercase().contains(needle)
        || f.foundry.to_lowercase().contains(needle)
        || f.faces.iter().any(|x| x.ps_name.to_lowercase().contains(needle) || x.full_name.to_lowercase().contains(needle))
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
        let collection = match q.collection_id {
            Some(id) => Some(keys_where(&db, "SELECT family_key FROM collection_items WHERE collection_id = ?1", id)?),
            None => None,
        };
        let tagged = match &q.tag {
            Some(t) => Some(keys_where(&db, "SELECT family_key FROM family_tags WHERE tag = ?1", t)?),
            None => None,
        };
        let needle = q.search.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_lowercase);

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
            {
                continue;
            }
            let fam = build_family(key, &rows, licenses.get(key).copied().unwrap_or(License::Unknown));
            // System-only families stay out of the library lists unless asked for (or collected/tagged/picked).
            if fam.system && q.filter != FamilyFilter::System && collection.is_none() && tagged.is_none() && q.keys.is_none() {
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
            if !keep || q.license.is_some_and(|l| l != fam.license) || needle.as_deref().is_some_and(|n| !matches_search(&fam, n)) {
                continue;
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
        }
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
        let family = build_family(key, &rows, info.label);
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
            family,
        })
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
            "SELECT c.id, c.name, c.parent_id, c.builtin, (SELECT COUNT(*) FROM collection_items WHERE collection_id = c.id)
             FROM collections c ORDER BY c.builtin IS NULL, c.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            let builtin: Option<String> = r.get(3)?;
            Ok(Collection {
                id: r.get(0)?,
                name: r.get(1)?,
                parent_id: r.get(2)?,
                count: r.get(4)?,
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
        set_license_row(&db, family_key, label, file, note.unwrap_or(""))?;
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
            self.toggle_project_set(id, false, true)?;
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
