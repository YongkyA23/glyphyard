//! Auto-organize planner. Produces a list of changes; applying them is the caller's job.
//! Renames touch only the file name — the font's internal names are never edited.

use crate::FaceInfo;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub hash: String,
    /// Unix seconds.
    pub modified: i64,
    /// Empty when the file could not be parsed.
    pub faces: Vec<FaceInfo>,
    /// The watched folder this file lives under, used as the root for tidy moves.
    pub home: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Case {
    ExactCopy,
    DifferentVersion,
    OtherFormat,
    NameClash,
    VariableStatic,
    InSystem,
    Junk,
    Tidy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
    /// Send to Recycle Bin / Trash; `kept` is the identical copy that stays.
    Trash { path: PathBuf, kept: PathBuf },
    Rename { from: PathBuf, to: PathBuf },
    KeepAsAlternate { path: PathBuf, preferred: PathBuf },
    MoveAside { from: PathBuf, to: PathBuf },
    Group { family: String, paths: Vec<PathBuf> },
    Skip { path: PathBuf },
    Quarantine { from: PathBuf, to: PathBuf },
    Move { from: PathBuf, to: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub case: Case,
    pub action: Action,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub tidy: bool,
    pub quarantine_dir: PathBuf,
    pub aside_dir: PathBuf,
}

const JUNK_NAMES: [&str; 3] = ["thumbs.db", ".ds_store", "desktop.ini"];

pub fn is_junk_path(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
    name.starts_with("._")
        || JUNK_NAMES.contains(&name.as_str())
        || path.components().any(|c| c.as_os_str().eq_ignore_ascii_case("__MACOSX"))
}

fn ext(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}

fn file_name(path: &Path) -> String {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()
}

/// Characters Windows or macOS reject in file names.
pub fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect::<String>()
        .trim_end_matches(['.', ' '])
        .to_string()
}

/// `Montserrat-Bold v8.000 (Studio A).otf`
pub fn versioned_name(ps_name: &str, version: Option<&str>, suffix: Option<&str>, ext: &str) -> String {
    let mut s = ps_name.to_string();
    if let Some(v) = version {
        s.push_str(&format!(" v{v}"));
    }
    if let Some(x) = suffix {
        s.push_str(&format!(" ({x})"));
    }
    format!("{}.{}", sanitize(&s), ext)
}

fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn primary(f: &ScannedFile) -> &FaceInfo {
    &f.faces[0]
}

/// Builds the plan. `system_ps_names` are lowercase PostScript names already installed system-wide.
pub fn plan(files: &[ScannedFile], system_ps_names: &HashSet<String>, opts: &Options) -> Vec<Change> {
    let mut changes = Vec::new();
    // Final file name for each kept file after renames (index into `files`).
    let mut renamed: HashMap<usize, String> = HashMap::new();
    let mut kept: Vec<usize> = Vec::new();

    // 1. Junk and unparsable files.
    let mut candidates = Vec::new();
    for (i, f) in files.iter().enumerate() {
        if is_junk_path(&f.path) || f.faces.is_empty() {
            let to = opts.quarantine_dir.join(format!("{}-{}", &f.hash[..8.min(f.hash.len())], file_name(&f.path)));
            let note = if f.faces.is_empty() && !is_junk_path(&f.path) { "not a readable font file" } else { "not a real font file" };
            changes.push(Change { case: Case::Junk, action: Action::Quarantine { from: f.path.clone(), to }, note: note.into() });
        } else {
            candidates.push(i);
        }
    }

    // 2. Exact copies: keep the one in a watched folder, then the oldest, then the shortest path.
    let mut by_hash: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for &i in &candidates {
        by_hash.entry(files[i].hash.as_str()).or_default().push(i);
    }
    let mut unique = Vec::new();
    for group in by_hash.into_values() {
        let mut group = group;
        group.sort_by_key(|&i| {
            let f = &files[i];
            (f.home.is_none(), f.modified, f.path.as_os_str().len(), f.path.clone())
        });
        let keeper = group[0];
        for &i in &group[1..] {
            changes.push(Change {
                case: Case::ExactCopy,
                action: Action::Trash { path: files[i].path.clone(), kept: files[keeper].path.clone() },
                note: "identical file is kept elsewhere".into(),
            });
        }
        unique.push(keeper);
    }

    // 3. Web-only formats and 4. fonts the system already has.
    let mut rest = Vec::new();
    for i in unique {
        let f = &files[i];
        if matches!(ext(&f.path).as_str(), "woff" | "woff2") {
            changes.push(Change {
                case: Case::OtherFormat,
                action: Action::MoveAside { from: f.path.clone(), to: opts.aside_dir.join(file_name(&f.path)) },
                note: "web-only format, not for desktop use".into(),
            });
        } else if system_ps_names.contains(&primary(f).ps_name.to_lowercase()) {
            changes.push(Change {
                case: Case::InSystem,
                action: Action::Skip { path: f.path.clone() },
                note: format!("{} is already installed in the system", primary(f).full_name),
            });
        } else {
            rest.push(i);
        }
    }

    // 5. Same PostScript name: clashes, versions, formats.
    let mut by_ps: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for &i in &rest {
        by_ps.entry(primary(&files[i]).ps_name.to_lowercase()).or_default().push(i);
    }
    for group in by_ps.into_values() {
        kept.extend(&group);
        if group.len() < 2 {
            continue;
        }
        // Identity = who made it. Different makers under one name is a clash.
        let mut by_maker: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
        for &i in &group {
            let p = primary(&files[i]);
            by_maker.entry((p.foundry.to_lowercase(), p.designer.to_lowercase())).or_default().push(i);
        }
        let mut makers: Vec<Vec<usize>> = by_maker.into_values().collect();
        makers.sort_by_key(|m| std::cmp::Reverse(m.len()));
        let clash = makers.len() > 1;
        for (m, members) in makers.iter().enumerate() {
            let suffix = (clash && m > 0).then(|| {
                let p = primary(&files[members[0]]);
                if p.foundry.is_empty() { p.designer.clone() } else { p.foundry.clone() }
            });
            plan_versions(files, members, suffix.as_deref(), &mut changes, &mut renamed);
        }
    }

    // 6. A variable font next to static styles of the same family.
    let mut by_family: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for &i in &kept {
        let p = primary(&files[i]);
        by_family.entry((p.family.to_lowercase(), p.foundry.to_lowercase())).or_default().push(i);
    }
    for members in by_family.values() {
        let var = members.iter().any(|&i| primary(&files[i]).is_variable);
        let stat = members.iter().any(|&i| !primary(&files[i]).is_variable);
        if var && stat {
            changes.push(Change {
                case: Case::VariableStatic,
                action: Action::Group {
                    family: primary(&files[members[0]]).family.clone(),
                    paths: members.iter().map(|&i| files[i].path.clone()).collect(),
                },
                note: "variable and static versions of one family: activate only one kind".into(),
            });
        }
    }

    // 7. Tidy into Foundry/Family/.
    if opts.tidy {
        for &i in &kept {
            let f = &files[i];
            let Some(home) = &f.home else { continue };
            let p = primary(f);
            let foundry = if p.foundry.is_empty() { "Unknown foundry".to_string() } else { sanitize(&p.foundry) };
            let name = renamed.get(&i).cloned().unwrap_or_else(|| file_name(&f.path));
            let target = home.join(foundry).join(sanitize(&p.family)).join(name);
            if target == f.path {
                continue;
            }
            let existing = changes.iter_mut().find(|c| matches!(&c.action, Action::Rename { from, .. } if *from == f.path));
            match existing {
                Some(Change { action: Action::Rename { to, .. }, .. }) => *to = target,
                _ => changes.push(Change {
                    case: Case::Tidy,
                    action: Action::Move { from: f.path.clone(), to: target },
                    note: "moved into Foundry / Family".into(),
                }),
            }
        }
    }

    changes
}

fn plan_versions(
    files: &[ScannedFile],
    members: &[usize],
    suffix: Option<&str>,
    changes: &mut Vec<Change>,
    renamed: &mut HashMap<usize, String>,
) {
    let mut by_version: BTreeMap<Vec<u64>, Vec<usize>> = BTreeMap::new();
    for &i in members {
        by_version.entry(version_key(&primary(&files[i]).version)).or_default().push(i);
    }
    let many_versions = by_version.len() > 1;
    let newest = by_version.keys().last().cloned();

    for (vkey, same) in &by_version {
        // Prefer OTF among same-version files; other formats stay as alternates.
        let mut same = same.clone();
        same.sort_by_key(|&i| (ext(&files[i].path) != "otf", files[i].path.clone()));
        let preferred = same[0];
        let mut same_format_seen = vec![ext(&files[preferred].path)];
        for &i in &same[1..] {
            let e = ext(&files[i].path);
            if same_format_seen.contains(&e) {
                // Same name, version and format but different bytes.
                let f = &files[i];
                let p = primary(f);
                let to_name = versioned_name(&p.ps_name, Some(&p.version), Some(&f.hash[..8.min(f.hash.len())]), &e);
                push_rename(files, i, to_name, Case::NameClash, "same name and version, but a different file", changes, renamed);
            } else {
                same_format_seen.push(e);
                changes.push(Change {
                    case: Case::OtherFormat,
                    action: Action::KeepAsAlternate { path: files[i].path.clone(), preferred: files[preferred].path.clone() },
                    note: "same font in another format; the OTF is used".into(),
                });
            }
        }

        let p = primary(&files[preferred]);
        let version = many_versions.then_some(p.version.as_str());
        if version.is_none() && suffix.is_none() {
            continue;
        }
        let to_name = versioned_name(&p.ps_name, version, suffix, &ext(&files[preferred].path));
        let (case, note) = match (suffix.is_some(), many_versions, Some(vkey) == newest.as_ref()) {
            (true, _, _) => (Case::NameClash, "same name as a different font; never active together".to_string()),
            (false, true, true) => (Case::DifferentVersion, "newest version, used by default".to_string()),
            _ => (Case::DifferentVersion, "older version; never active at the same time as the newest".to_string()),
        };
        push_rename(files, preferred, to_name, case, &note, changes, renamed);
    }
}

fn push_rename(
    files: &[ScannedFile],
    i: usize,
    to_name: String,
    case: Case,
    note: &str,
    changes: &mut Vec<Change>,
    renamed: &mut HashMap<usize, String>,
) {
    let from = &files[i].path;
    renamed.insert(i, to_name.clone());
    if file_name(from) == to_name {
        return;
    }
    let to = from.with_file_name(&to_name);
    changes.push(Change { case, action: Action::Rename { from: from.clone(), to }, note: note.into() });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(ps: &str, family: &str, version: &str, foundry: &str, variable: bool) -> FaceInfo {
        FaceInfo {
            index: 0,
            ps_name: ps.into(),
            family: family.into(),
            style: "Bold".into(),
            full_name: ps.replace('-', " "),
            version: version.into(),
            foundry: foundry.into(),
            designer: String::new(),
            weight: 700.0,
            italic: false,
            is_variable: variable,
            axes: vec![],
            features: vec![],
            glyph_count: 500,
        }
    }

    fn file(path: &str, hash: &str, modified: i64, faces: Vec<FaceInfo>) -> ScannedFile {
        ScannedFile { path: path.into(), hash: hash.into(), modified, faces, home: Some("/fonts".into()) }
    }

    fn opts(tidy: bool) -> Options {
        Options { tidy, quarantine_dir: "/q".into(), aside_dir: "/web".into() }
    }

    fn cases(changes: &[Change]) -> Vec<Case> {
        changes.iter().map(|c| c.case).collect()
    }

    #[test]
    fn exact_copies_keep_oldest() {
        let f = face("Montserrat-Bold", "Montserrat", "8.000", "ULA", false);
        let files = vec![
            file("/fonts/b/Montserrat-Bold.otf", "aaaa1111", 20, vec![f.clone()]),
            file("/fonts/a/Montserrat-Bold.otf", "aaaa1111", 10, vec![f]),
        ];
        let plan = plan(&files, &HashSet::new(), &opts(false));
        assert_eq!(cases(&plan), vec![Case::ExactCopy]);
        assert_eq!(
            plan[0].action,
            Action::Trash { path: "/fonts/b/Montserrat-Bold.otf".into(), kept: "/fonts/a/Montserrat-Bold.otf".into() }
        );
    }

    #[test]
    fn versions_get_version_in_name() {
        let files = vec![
            file("/fonts/Montserrat-Bold.otf", "new00000", 1, vec![face("Montserrat-Bold", "Montserrat", "8.000", "ULA", false)]),
            file("/fonts/old/Montserrat-Bold.otf", "old00000", 1, vec![face("Montserrat-Bold", "Montserrat", "7.200", "ULA", false)]),
        ];
        let plan = plan(&files, &HashSet::new(), &opts(false));
        let renames: Vec<_> = plan.iter().filter_map(|c| match &c.action {
            Action::Rename { to, .. } => Some((c.case, to.clone(), c.note.clone())),
            _ => None,
        }).collect();
        assert_eq!(renames.len(), 2);
        assert!(renames.iter().any(|(_, to, note)| to.ends_with("Montserrat-Bold v8.000.otf") && note.contains("newest")));
        assert!(renames.iter().any(|(_, to, _)| to.ends_with("Montserrat-Bold v7.200.otf")));
        assert!(renames.iter().all(|(c, _, _)| *c == Case::DifferentVersion));
    }

    #[test]
    fn different_makers_are_a_clash() {
        let files = vec![
            file("/fonts/a/Unnamed-Regular.otf", "h1000000", 1, vec![face("Unnamed-Regular", "Unnamed", "1.000", "AAAA", false)]),
            file("/fonts/a2/Unnamed-Regular.otf", "h1100000", 1, vec![face("Unnamed-Regular", "Unnamed", "1.000", "AAAA", false)]),
            file("/fonts/b/Unnamed-Regular.otf", "h2000000", 1, vec![face("Unnamed-Regular", "Unnamed", "1.000", "Studio A", false)]),
        ];
        let plan = plan(&files, &HashSet::new(), &opts(false));
        assert!(plan.iter().any(|c| c.case == Case::NameClash
            && matches!(&c.action, Action::Rename { to, .. } if to.ends_with("Unnamed-Regular (Studio A).otf"))));
    }

    #[test]
    fn otf_preferred_over_ttf() {
        let f = face("Syne-Bold", "Syne", "2.000", "BJM", false);
        let files = vec![
            file("/fonts/Syne-Bold.ttf", "t0000000", 1, vec![f.clone()]),
            file("/fonts/Syne-Bold.otf", "o0000000", 1, vec![f]),
        ];
        let plan = plan(&files, &HashSet::new(), &opts(false));
        assert_eq!(
            plan[0].action,
            Action::KeepAsAlternate { path: "/fonts/Syne-Bold.ttf".into(), preferred: "/fonts/Syne-Bold.otf".into() }
        );
    }

    #[test]
    fn junk_system_web_and_variable_groups() {
        let files = vec![
            file("/fonts/__MACOSX/._Lora.ttf", "j0000000", 1, vec![]),
            file("/fonts/Thumbs.db", "j1000000", 1, vec![]),
            file("/fonts/Arial.ttf", "s0000000", 1, vec![face("ArialMT", "Arial", "7.00", "TMC", false)]),
            file("/fonts/Lora.woff2", "w0000000", 1, vec![face("Lora-Regular", "Lora", "3.0", "CYRE", false)]),
            file("/fonts/Fraunces[wght].ttf", "v0000000", 1, vec![face("Fraunces-Regular", "Fraunces", "1.000", "UNDR", true)]),
            file("/fonts/Fraunces-Bold.otf", "v1000000", 1, vec![face("Fraunces-Bold", "Fraunces", "1.000", "UNDR", false)]),
        ];
        let system: HashSet<String> = ["arialmt".to_string()].into();
        let plan = plan(&files, &system, &opts(false));
        let c = cases(&plan);
        assert_eq!(c.iter().filter(|&&x| x == Case::Junk).count(), 2);
        assert!(c.contains(&Case::InSystem));
        assert!(c.contains(&Case::OtherFormat));
        assert!(c.contains(&Case::VariableStatic));
    }

    #[test]
    fn tidy_merges_with_rename() {
        let files = vec![
            file("/fonts/x/Montserrat-Bold.otf", "n0000000", 1, vec![face("Montserrat-Bold", "Montserrat", "8.000", "ULA", false)]),
            file("/fonts/y/Montserrat-Bold.otf", "o0000000", 1, vec![face("Montserrat-Bold", "Montserrat", "7.200", "ULA", false)]),
            file("/fonts/Lora-Regular.otf", "l0000000", 1, vec![face("Lora-Regular", "Lora", "3.0", "CYRE", false)]),
        ];
        let plan = plan(&files, &HashSet::new(), &opts(true));
        assert!(plan.iter().any(|c| matches!(&c.action,
            Action::Rename { to, .. } if to == Path::new("/fonts/ULA/Montserrat/Montserrat-Bold v8.000.otf"))));
        assert!(plan.iter().any(|c| c.case == Case::Tidy && matches!(&c.action,
            Action::Move { to, .. } if to == Path::new("/fonts/CYRE/Lora/Lora-Regular.otf"))));
    }

    #[test]
    fn sanitizes_file_names() {
        assert_eq!(versioned_name("A/B:C", Some("1.0"), None, "otf"), "A_B_C v1.0.otf");
    }
}
