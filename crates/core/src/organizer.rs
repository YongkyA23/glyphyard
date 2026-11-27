//! The person's synced collections and project sets, and the merge both the server and the desktop run.
//!
//! **Timestamps** are RFC 3339 strings in one canonical form, UTC with microseconds and `Z`
//! (`2026-11-27T08:15:00.123456Z`, what [`timestamp`] writes), so they compare correctly as text.
//! [`merge`] and [`prune`] compare them as text and assume that form; run untrusted input through
//! [`normalize`] first, which rewrites any RFC 3339 offset to it and rejects anything else.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

/// The uid of the Favorites builtin. Its name and parent are ignored on apply, and it never collapses.
pub const FAVORITES: &str = "favorites";
/// How long tombstones and removed families are kept before [`prune`] drops them.
pub const KEEP_DAYS: u32 = 180;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct OrganizerDoc {
    pub collections: Vec<SyncCollection>,
    pub sets: Vec<SyncSet>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SyncCollection {
    pub uid: String,
    pub name: String,
    pub parent: Option<String>,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub items: Vec<SyncItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SyncSet {
    pub uid: String,
    pub name: String,
    pub note: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub items: Vec<SyncItem>,
}

/// One family in a collection or set. `removed` keeps the removal so an older add can't bring it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SyncItem {
    pub key: String,
    pub at: String,
    pub removed: bool,
}

/// The canonical timestamp form.
pub fn timestamp(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Rewrites every timestamp to the canonical form. `Err` names the first one that isn't RFC 3339.
pub fn normalize(doc: &OrganizerDoc) -> Result<OrganizerDoc, String> {
    let mut doc = doc.clone();
    let stamps = doc.collections.iter_mut().flat_map(Entry::stamps).chain(doc.sets.iter_mut().flat_map(Entry::stamps));
    for s in stamps {
        let dt = DateTime::parse_from_rfc3339(s).map_err(|_| format!("`{s}` is not an RFC 3339 timestamp"))?;
        *s = timestamp(dt.with_timezone(&Utc));
    }
    Ok(doc)
}

/// Merges two documents into one sorted by uid, with items sorted by key.
///
/// - Per uid, the newer `updated_at` wins the own fields. A tie goes to the greater name, then to the smaller
///   parent (sets: note), so a child re-pointed by a collapse beats a stale copy of itself. `deleted_at` is the
///   max of both, and the item is deleted while `deleted_at >= updated_at`. A `deleted_at` older than
///   `updated_at` can never matter again (`updated_at` only grows), so it is dropped to `None`.
/// - Families merge per key: the newer `at` wins, a tie goes to `removed`.
/// - Live items of one kind with the same name and parent collapse into the smallest uid: their families are
///   unioned into it, the others are tombstoned at their own `updated_at` (so a later edit revives them), and
///   children pointing at any of them, tombstones included, are re-pointed to it. Parent loops stay as they
///   are; see [`loop_cuts`].
///
/// Pure, deterministic, commutative and idempotent, and associative except when a rename or delete races a
/// same-name collapse: the survivor may then keep the families the other one brought, depending on merge order.
/// Families are only ever added there, never lost.
pub fn merge(a: &OrganizerDoc, b: &OrganizerDoc) -> OrganizerDoc {
    OrganizerDoc { collections: merge_kind(&a.collections, &b.collections), sets: merge_kind(&a.sets, &b.sets) }
}

/// Drops tombstoned collections and sets, and removed families, older than `keep_days` before `now`.
/// A `now` that isn't RFC 3339 drops nothing.
pub fn prune(doc: &OrganizerDoc, now: &str, keep_days: u32) -> OrganizerDoc {
    let now = DateTime::parse_from_rfc3339(now).map(|t| t.with_timezone(&Utc));
    let Some(cutoff) = now.ok().and_then(|t| t.checked_sub_signed(Duration::days(keep_days.into()))) else { return doc.clone() };
    let cutoff = timestamp(cutoff);
    OrganizerDoc { collections: prune_kind(&doc.collections, &cutoff), sets: prune_kind(&doc.sets, &cutoff) }
}

/// What merge needs from a collection or a set.
trait Entry: Clone {
    fn uid(&self) -> &str;
    fn updated_at(&self) -> &str;
    fn deleted_at(&mut self) -> &mut Option<String>;
    fn items(&mut self) -> &mut Vec<SyncItem>;
    /// The own fields, for the same-`updated_at` tie-break.
    fn fields(&self) -> (&str, Reverse<Option<&str>>);
    /// Live entries with equal keys are duplicates; `None` never collapses.
    fn dup_key(&self) -> Option<(String, Option<String>)>;
    fn parent(&mut self) -> Option<&mut Option<String>>;
    fn parent_of(&self) -> Option<&str>;

    fn deleted(&mut self) -> bool {
        let at = self.updated_at().to_string();
        self.deleted_at().as_ref().is_some_and(|d| *d >= at)
    }

    fn stamps(&mut self) -> Vec<&mut String>;
}

macro_rules! entry_common {
    () => {
        fn uid(&self) -> &str {
            &self.uid
        }
        fn updated_at(&self) -> &str {
            &self.updated_at
        }
        fn deleted_at(&mut self) -> &mut Option<String> {
            &mut self.deleted_at
        }
        fn items(&mut self) -> &mut Vec<SyncItem> {
            &mut self.items
        }
        fn stamps(&mut self) -> Vec<&mut String> {
            let mut v = vec![&mut self.updated_at];
            v.extend(self.deleted_at.as_mut());
            v.extend(self.items.iter_mut().map(|i| &mut i.at));
            v
        }
    };
}

impl Entry for SyncCollection {
    entry_common!();
    fn fields(&self) -> (&str, Reverse<Option<&str>>) {
        (&self.name, Reverse(self.parent.as_deref()))
    }
    fn dup_key(&self) -> Option<(String, Option<String>)> {
        (self.uid != FAVORITES).then(|| (self.name.clone(), self.parent.clone()))
    }
    fn parent(&mut self) -> Option<&mut Option<String>> {
        Some(&mut self.parent)
    }
    fn parent_of(&self) -> Option<&str> {
        self.parent.as_deref()
    }
}

impl Entry for SyncSet {
    entry_common!();
    fn fields(&self) -> (&str, Reverse<Option<&str>>) {
        (&self.name, Reverse(Some(&self.note)))
    }
    fn dup_key(&self) -> Option<(String, Option<String>)> {
        Some((self.name.clone(), None))
    }
    fn parent(&mut self) -> Option<&mut Option<String>> {
        None
    }
    fn parent_of(&self) -> Option<&str> {
        None
    }
}

/// Per key, the newer `at` wins and a tie goes to `removed`. Sorted by key.
fn merge_items<'a>(items: impl IntoIterator<Item = &'a SyncItem>) -> Vec<SyncItem> {
    let mut by_key: BTreeMap<&str, &SyncItem> = BTreeMap::new();
    for i in items {
        let cur = by_key.entry(&i.key).or_insert(i);
        if (&i.at, i.removed) > (&cur.at, cur.removed) {
            *cur = i;
        }
    }
    by_key.into_values().cloned().collect()
}

fn join<E: Entry>(x: E, y: E) -> E {
    let (mut won, mut lost) = if (x.updated_at(), x.fields()) >= (y.updated_at(), y.fields()) { (x, y) } else { (y, x) };
    let deleted_at = won.deleted_at().take().max(lost.deleted_at().take());
    *won.deleted_at() = deleted_at;
    *won.items() = merge_items(won.items().iter().chain(lost.items().iter()));
    won
}

fn merge_kind<E: Entry>(a: &[E], b: &[E]) -> Vec<E> {
    let mut by_uid: BTreeMap<String, E> = BTreeMap::new();
    for e in a.iter().chain(b) {
        let e = match by_uid.remove(e.uid()) {
            Some(cur) => join(cur, e.clone()),
            None => join(e.clone(), e.clone()),
        };
        by_uid.insert(e.uid().to_string(), e);
    }
    collapse(&mut by_uid);
    by_uid
        .into_values()
        .map(|mut e| {
            if !e.deleted() {
                *e.deleted_at() = None;
            }
            e
        })
        .collect()
}

/// Collections whose parent must be read as the top level, one per parent loop. Concurrent moves can make a
/// loop (A under B on one computer, B under A on another); the doc keeps both moves, and in each loop the member
/// with the newest `updated_at` (then the greatest uid) is cut loose. Apply uses this; so does [`merge`].
pub fn loop_cuts(doc: &OrganizerDoc) -> BTreeSet<String> {
    cuts(&doc.collections)
}

fn cuts<'a, E: Entry + 'a>(entries: impl IntoIterator<Item = &'a E>) -> BTreeSet<String> {
    let nodes: BTreeMap<&str, (&str, Option<&str>)> = entries.into_iter().map(|e| (e.uid(), (e.updated_at(), e.parent_of()))).collect();
    let mut cut = BTreeSet::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for &start in nodes.keys() {
        let mut path: Vec<&str> = Vec::new();
        let mut cur = Some(start);
        while let Some(uid) = cur.filter(|u| !seen.contains(u)) {
            if let Some(i) = path.iter().position(|p| *p == uid) {
                let newest = path[i..].iter().max_by_key(|u| (nodes[**u].0, **u)).expect("a loop has members");
                cut.insert(newest.to_string());
                break;
            }
            path.push(uid);
            cur = nodes.get(uid).and_then(|n| n.1);
        }
        seen.extend(path);
    }
    cut
}

/// Repeats until nothing changes, so duplicates nested in duplicates collapse once their parents did.
// ponytail: one pass per nesting level, O(depth · n log n); fine for real trees, a 2 MiB doc nested thousands deep is slow.
fn collapse<E: Entry>(by_uid: &mut BTreeMap<String, E>) {
    loop {
        // A loop member that is cut loose compares as top level; its parent field is left alone.
        let cut = cuts(by_uid.values());
        let key = |e: &E| e.dup_key().map(|(name, parent)| (name, parent.filter(|_| !cut.contains(e.uid()))));
        // Uids iterate in order, so the first live one per key is the smallest.
        let mut target: BTreeMap<(String, Option<String>), String> = BTreeMap::new();
        for (uid, e) in by_uid.iter_mut() {
            if let (false, Some(k)) = (e.deleted(), key(e)) {
                target.entry(k).or_insert_with(|| uid.clone());
            }
        }
        // Tombstones alias too: a re-pointed child that loses a tie to a stale copy gets re-pointed again.
        let alias: BTreeMap<String, String> =
            by_uid.iter().filter_map(|(uid, e)| target.get(&key(e)?).filter(|t| *t < uid).map(|t| (uid.clone(), t.clone()))).collect();
        let mut changed = false;
        for (from, to) in &alias {
            let e = by_uid.get_mut(from).expect("alias of an entry");
            if e.deleted() {
                continue;
            }
            *e.deleted_at() = Some(e.updated_at().to_string());
            let moved = e.items().clone();
            let t = by_uid.get_mut(to).expect("alias target");
            *t.items() = merge_items(t.items().iter().chain(&moved));
            changed = true;
        }
        for e in by_uid.values_mut() {
            if let Some(parent) = e.parent() {
                if let Some(to) = parent.as_ref().and_then(|p| alias.get(p)) {
                    *parent = Some(to.clone());
                    changed = true;
                }
            }
        }
        if !changed {
            return;
        }
    }
}

fn prune_kind<E: Entry>(entries: &[E], cutoff: &str) -> Vec<E> {
    entries
        .iter()
        .cloned()
        .filter_map(|mut e| {
            if e.deleted() && e.deleted_at().as_deref().is_some_and(|d| d < cutoff) {
                return None;
            }
            e.items().retain(|i| !(i.removed && i.at.as_str() < cutoff));
            Some(e)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(sec: u32) -> String {
        format!("2026-01-01T00:{:02}:{:02}.000000Z", sec / 60, sec % 60)
    }

    fn item(key: &str, at: u32, removed: bool) -> SyncItem {
        SyncItem { key: key.into(), at: ts(at), removed }
    }

    fn col(uid: &str, name: &str, parent: Option<&str>, at: u32, items: Vec<SyncItem>) -> SyncCollection {
        SyncCollection { uid: uid.into(), name: name.into(), parent: parent.map(Into::into), updated_at: ts(at), deleted_at: None, items }
    }

    fn set(uid: &str, name: &str, at: u32, items: Vec<SyncItem>) -> SyncSet {
        SyncSet { uid: uid.into(), name: name.into(), note: String::new(), updated_at: ts(at), deleted_at: None, items }
    }

    fn doc(collections: Vec<SyncCollection>, sets: Vec<SyncSet>) -> OrganizerDoc {
        OrganizerDoc { collections, sets }
    }

    fn find<'a>(d: &'a OrganizerDoc, uid: &str) -> &'a SyncCollection {
        d.collections.iter().find(|c| c.uid == uid).unwrap_or_else(|| panic!("no {uid} in {d:?}"))
    }

    fn keys(items: &[SyncItem]) -> Vec<&str> {
        items.iter().filter(|i| !i.removed).map(|i| i.key.as_str()).collect()
    }

    fn deleted(c: &SyncCollection) -> bool {
        c.deleted_at.as_ref().is_some_and(|d| *d >= c.updated_at)
    }

    #[test]
    fn merge_is_commutative_and_keeps_concurrent_adds() {
        let a = doc(vec![col("c1", "Client X", None, 1, vec![item("Inter", 2, false)])], vec![set("s1", "Pitch", 1, vec![item("Lora", 4, false)])]);
        let b = doc(vec![col("c1", "Client X", None, 1, vec![item("Fraunces", 3, false)])], vec![set("s1", "Pitch", 1, vec![item("Lora", 5, true)])]);
        let m = merge(&a, &b);
        assert_eq!(m, merge(&b, &a));
        assert_eq!(keys(&find(&m, "c1").items), ["Fraunces", "Inter"]);
        assert_eq!(m.sets[0].items, [item("Lora", 5, true)], "the newer removal wins");
        // A tie on `at` goes to the removal, in either order.
        let tie_a = doc(vec![col("c1", "Client X", None, 1, vec![item("Inter", 2, false)])], vec![]);
        let tie_b = doc(vec![col("c1", "Client X", None, 1, vec![item("Inter", 2, true)])], vec![]);
        assert_eq!(merge(&tie_a, &tie_b).collections[0].items, [item("Inter", 2, true)]);
        assert_eq!(merge(&tie_b, &tie_a).collections[0].items, [item("Inter", 2, true)]);
        // A tie on `updated_at` goes to the greater fields, in either order.
        let r1 = doc(vec![col("c1", "Alpha", None, 7, vec![])], vec![]);
        let r2 = doc(vec![col("c1", "Beta", None, 7, vec![])], vec![]);
        assert_eq!(merge(&r1, &r2).collections[0].name, "Beta");
        assert_eq!(merge(&r2, &r1).collections[0].name, "Beta");
    }

    #[test]
    fn delete_wins_over_older_edit() {
        let edited = doc(vec![col("c1", "Renamed", None, 2, vec![])], vec![]);
        let mut gone = col("c1", "Client X", None, 1, vec![]);
        gone.deleted_at = Some(ts(3));
        let m = merge(&edited, &doc(vec![gone], vec![]));
        let c = find(&m, "c1");
        assert!(deleted(c));
        assert_eq!(c.deleted_at.as_deref(), Some(ts(3).as_str()));
        // A delete at the same instant as the edit still deletes.
        let mut same = col("c1", "Client X", None, 1, vec![]);
        same.deleted_at = Some(ts(2));
        assert!(deleted(find(&merge(&edited, &doc(vec![same], vec![])), "c1")));
    }

    #[test]
    fn newer_edit_after_delete_revives() {
        let mut gone = col("c1", "Client X", None, 1, vec![item("Inter", 1, false)]);
        gone.deleted_at = Some(ts(2));
        let back = doc(vec![col("c1", "Back", None, 3, vec![])], vec![]);
        let m = merge(&doc(vec![gone], vec![]), &back);
        let c = find(&m, "c1");
        assert!(!deleted(c));
        assert_eq!(c.name, "Back");
        assert_eq!(c.deleted_at, None, "a delete older than the edit no longer matters");
        assert_eq!(keys(&c.items), ["Inter"]);
    }

    #[test]
    fn same_name_siblings_collapse() {
        // Both computers made "Client X" with a "Logos" inside, and the laptop also a "Drafts".
        let laptop = doc(
            vec![
                col("bbb", "Client X", None, 1, vec![item("Inter", 2, false)]),
                col("kid-b", "Logos", Some("bbb"), 1, vec![item("Lora", 2, false)]),
                col("drafts", "Drafts", Some("bbb"), 1, vec![]),
            ],
            vec![set("s-b", "Pitch", 1, vec![item("Inter", 1, false)])],
        );
        let mac = doc(
            vec![col("aaa", "Client X", None, 4, vec![item("Fraunces", 3, false)]), col("kid-a", "Logos", Some("aaa"), 1, vec![])],
            vec![set("s-a", "Pitch", 1, vec![item("Lora", 1, false)])],
        );
        let m = merge(&laptop, &mac);
        assert_eq!(m, merge(&mac, &laptop));
        let kept = find(&m, "aaa");
        assert!(!deleted(kept));
        assert_eq!(keys(&kept.items), ["Fraunces", "Inter"]);
        let gone = find(&m, "bbb");
        assert!(deleted(gone));
        assert_eq!(gone.deleted_at.as_deref(), Some(ts(1).as_str()), "tombstoned at its own updated_at");
        assert_eq!(find(&m, "drafts").parent.as_deref(), Some("aaa"), "children follow");
        assert!(!deleted(find(&m, "kid-a")));
        assert_eq!(keys(&find(&m, "kid-a").items), ["Lora"], "nested duplicates collapse too");
        assert!(deleted(find(&m, "kid-b")));
        assert_eq!(m.sets.iter().filter(|s| s.deleted_at.is_none()).count(), 1);
        assert_eq!(keys(&m.sets[0].items), ["Inter", "Lora"]);
        // Merging either side in again changes nothing, though their children still point at "bbb".
        assert_eq!(merge(&m, &laptop), m);
        assert_eq!(merge(&mac, &m), m);
        assert_eq!(merge(&m, &m), m);
        // Favorites never collapses.
        let favs = doc(vec![col(FAVORITES, "Client X", None, 1, vec![]), col("zzz", "Client X", None, 1, vec![])], vec![]);
        assert!(merge(&favs, &favs).collections.iter().all(|c| !deleted(c)));
    }

    #[test]
    fn merge_is_idempotent_and_associative() {
        let mut gone = col("c2", "Old", None, 1, vec![]);
        gone.deleted_at = Some(ts(5));
        let a = doc(vec![col("c1", "Client X", None, 1, vec![item("Inter", 2, false)]), gone], vec![set("s1", "Pitch", 1, vec![])]);
        let b = doc(
            vec![col("c1", "Client X", None, 3, vec![item("Inter", 4, true)]), col("c2", "Old", None, 2, vec![item("Lora", 2, false)])],
            vec![set("s1", "Pitch v2", 6, vec![item("Lora", 6, false)])],
        );
        let c = doc(vec![col("c9", "Client X", None, 2, vec![item("Fraunces", 7, false)]), col("c3", "Logos", Some("c9"), 1, vec![])], vec![]);
        let all = [&a, &b, &c];
        let m = merge(&merge(&a, &b), &c);
        assert_eq!(m, merge(&a, &merge(&b, &c)));
        for (x, y, z) in [(0, 1, 2), (0, 2, 1), (1, 0, 2), (1, 2, 0), (2, 0, 1), (2, 1, 0)] {
            assert_eq!(merge(&merge(all[x], all[y]), all[z]), m, "order {x}{y}{z}");
        }
        assert_eq!(merge(&m, &m), m);
        assert_eq!(merge(&a, &a), merge(&merge(&a, &a), &a));
        assert!(deleted(find(&m, "c2")));
        assert!(deleted(find(&m, "c9")));
        assert_eq!(find(&m, "c3").parent.as_deref(), Some("c1"));
        assert_eq!(find(&m, "c1").items, [item("Fraunces", 7, false), item("Inter", 4, true)]);
    }

    /// Tiny LCG so the laws are checked over many shapes without a dependency.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) % n
        }

        fn items(&mut self) -> Vec<SyncItem> {
            (0..self.below(3)).map(|_| item(["k1", "k2", "k3"][self.below(3) as usize], self.below(6) as u32, self.below(2) == 0)).collect()
        }

        fn stamp(&mut self) -> (u32, Option<String>) {
            (self.below(6) as u32, (self.below(3) == 0).then(|| ts(self.below(6) as u32)))
        }

        /// `dup_names`: names repeat across uids, so same-name collapse happens.
        fn doc(&mut self, dup_names: bool) -> OrganizerDoc {
            let uids = ["a", "b", "c", "d", FAVORITES];
            let collections = (0..self.below(5))
                .map(|_| {
                    let uid = uids[self.below(5) as usize];
                    let name = if dup_names { ["X", "Y"][self.below(2) as usize] } else { uid };
                    let parent = [None, Some("a"), Some("b")][self.below(3) as usize];
                    let (at, deleted_at) = self.stamp();
                    let items = self.items();
                    SyncCollection { deleted_at, ..col(uid, name, parent, at, items) }
                })
                .collect();
            let sets = (0..self.below(3))
                .map(|_| {
                    let uid = uids[self.below(4) as usize];
                    let name = if dup_names { "P" } else { uid };
                    let (at, deleted_at) = self.stamp();
                    let note = ["", "n"][self.below(2) as usize].to_string();
                    let items = self.items();
                    SyncSet { deleted_at, note, ..set(uid, name, at, items) }
                })
                .collect();
            doc(collections, sets)
        }
    }

    #[test]
    fn merge_laws_hold_on_random_docs() {
        let mut rng = Rng(7);
        for round in 0..3000 {
            let (a, b, c) = (rng.doc(true), rng.doc(true), rng.doc(true));
            let ab = merge(&a, &b);
            assert_eq!(ab, merge(&b, &a), "commutative, round {round}");
            assert_eq!(merge(&ab, &ab), ab, "idempotent, round {round}");
            assert_eq!(merge(&ab, &b), ab, "absorbs a side already in it, round {round}");
            assert_eq!(merge(&merge(&a, &b), &c), merge(&merge(&b, &a), &c), "round {round}");
            // Associative whenever no rename or delete races a same-name collapse (see `merge`).
            let (a, b, c) = (rng.doc(false), rng.doc(false), rng.doc(false));
            assert_eq!(merge(&merge(&a, &b), &c), merge(&a, &merge(&b, &c)), "associative, round {round}");
        }
    }

    #[test]
    fn prune_drops_old_tombstones() {
        let at = |s: &str| format!("{s}T00:00:00.000000Z");
        let mut old = col("old", "Old", None, 0, vec![]);
        (old.updated_at, old.deleted_at) = (at("2025-12-01"), Some(at("2026-01-01")));
        let mut recent = col("recent", "Recent", None, 0, vec![]);
        (recent.updated_at, recent.deleted_at) = (at("2026-05-01"), Some(at("2026-06-01")));
        let mut live = col("live", "Live", None, 0, vec![]);
        live.updated_at = at("2025-01-01");
        live.items = vec![
            SyncItem { key: "gone".into(), at: at("2025-01-01"), removed: true },
            SyncItem { key: "kept".into(), at: at("2025-01-01"), removed: false },
            SyncItem { key: "just-removed".into(), at: at("2026-06-01"), removed: true },
        ];
        let mut old_set = set("s", "Old set", 0, vec![]);
        old_set.deleted_at = Some(ts(1));
        let p = prune(&doc(vec![old, recent, live], vec![old_set]), &at("2026-07-01"), KEEP_DAYS);
        assert_eq!(p.collections.iter().map(|c| c.uid.as_str()).collect::<Vec<_>>(), ["recent", "live"]);
        assert_eq!(p.collections[1].items.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["kept", "just-removed"]);
        assert!(p.sets.is_empty());
    }

    #[test]
    fn normalize_rewrites_timestamps_to_utc_micros() {
        let mut c = col("c1", "X", None, 0, vec![SyncItem { key: "k".into(), at: "2026-01-01T07:00:00+07:00".into(), removed: false }]);
        c.updated_at = "2026-01-01T00:00:01Z".into();
        c.deleted_at = Some("2026-01-01T00:00:01.5Z".into());
        let n = normalize(&doc(vec![c], vec![])).unwrap();
        assert_eq!(n.collections[0].updated_at, "2026-01-01T00:00:01.000000Z");
        assert_eq!(n.collections[0].deleted_at.as_deref(), Some("2026-01-01T00:00:01.500000Z"));
        assert_eq!(n.collections[0].items[0].at, "2026-01-01T00:00:00.000000Z");
        let bad = doc(vec![], vec![SyncSet { updated_at: "yesterday".into(), ..set("s", "S", 0, vec![]) }]);
        assert!(normalize(&bad).unwrap_err().contains("yesterday"));
    }
}
