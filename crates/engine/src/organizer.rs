//! Collections and project sets as the person's synced organizer document (`typeshelf_core::organizer`).
//!
//! Every local edit runs between [`Local::read`] and [`record`] in one transaction: the difference stamps what
//! changed (a new row gets a uid, a renamed one a later `updated_at`, an added family an `added_at`, a removed
//! one an `organizer_removed` row, a deleted row a tombstone) and counts one more unsynced change. Recent never
//! syncs (its uid stays NULL). Applying a document from the server is not a local change.

use crate::{Engine, EngineError, Result, UiEvent};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet};
use typeshelf_core::organizer::{self as core, OrganizerDoc, SyncCollection, SyncItem, SyncSet, FAVORITES, KEEP_DAYS};

/// Now, in the organizer's canonical timestamp form.
pub(crate) fn now() -> String {
    core::timestamp(Utc::now())
}

/// A stamp for a change after `prev`: now, or 1 µs after `prev` when that is later (another computer's clock
/// ran ahead), so the change still wins. A user delete therefore always has `deleted_at > updated_at`, which
/// merge reads as a delete and not as a same-name collapse.
fn later(prev: &str, now: &str) -> String {
    let next = DateTime::parse_from_rfc3339(prev).ok().map(|p| core::timestamp(p.with_timezone(&Utc) + Duration::microseconds(1)));
    next.filter(|n| n.as_str() > now).unwrap_or_else(|| now.to_string())
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Collection,
    Set,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Collection => "collection",
            Kind::Set => "set",
        }
    }
    fn table(self) -> &'static str {
        match self {
            Kind::Collection => "collections",
            Kind::Set => "project_sets",
        }
    }
    fn items(self) -> &'static str {
        match self {
            Kind::Collection => "collection_items",
            Kind::Set => "project_set_items",
        }
    }
    fn owner(self) -> &'static str {
        match self {
            Kind::Collection => "collection_id",
            Kind::Set => "set_id",
        }
    }
}

/// A collection or a set: `extra` is the parent uid (collections) or the note (sets).
#[derive(Debug, Clone, Default, PartialEq)]
struct Node {
    uid: String,
    name: String,
    extra: Option<String>,
    updated_at: String,
    deleted_at: Option<String>,
    items: Vec<SyncItem>,
}

impl Node {
    fn live(&self) -> BTreeMap<&str, &str> {
        self.items.iter().filter(|i| !i.removed).map(|i| (i.key.as_str(), i.at.as_str())).collect()
    }
    fn from_collection(c: SyncCollection) -> Node {
        Node { uid: c.uid, name: c.name, extra: c.parent, updated_at: c.updated_at, deleted_at: c.deleted_at, items: c.items }
    }
    fn from_set(s: SyncSet) -> Node {
        Node { uid: s.uid, name: s.name, extra: Some(s.note), updated_at: s.updated_at, deleted_at: s.deleted_at, items: s.items }
    }
    fn collection(self) -> SyncCollection {
        SyncCollection { uid: self.uid, name: self.name, parent: self.extra, updated_at: self.updated_at, deleted_at: self.deleted_at, items: self.items }
    }
    fn set(self) -> SyncSet {
        SyncSet { uid: self.uid, name: self.name, note: self.extra.unwrap_or_default(), updated_at: self.updated_at, deleted_at: self.deleted_at, items: self.items }
    }
    fn json(&self, kind: Kind) -> String {
        let n = self.clone();
        match kind {
            Kind::Collection => serde_json::to_string(&n.collection()),
            Kind::Set => serde_json::to_string(&n.set()),
        }
        .expect("organizer entries serialize")
    }
}

/// Collections (Recent left out) and project sets as stored, live rows and tombstones, uid order.
pub(crate) struct Local {
    collections: Vec<Node>,
    sets: Vec<Node>,
}

impl Local {
    pub(crate) fn read(conn: &Connection) -> rusqlite::Result<Local> {
        Ok(Local { collections: read_kind(conn, Kind::Collection)?, sets: read_kind(conn, Kind::Set)? })
    }

    fn of(&self, kind: Kind) -> &[Node] {
        match kind {
            Kind::Collection => &self.collections,
            Kind::Set => &self.sets,
        }
    }

    fn doc(self) -> OrganizerDoc {
        OrganizerDoc {
            collections: self.collections.into_iter().map(Node::collection).collect(),
            sets: self.sets.into_iter().map(Node::set).collect(),
        }
    }
}

fn read_kind(conn: &Connection, kind: Kind) -> rusqlite::Result<Vec<Node>> {
    let rows_sql = match kind {
        Kind::Collection => {
            "SELECT c.id, c.uid, c.name, p.uid, c.updated_at FROM collections c LEFT JOIN collections p ON p.id = c.parent_id
             WHERE c.uid IS NOT NULL"
        }
        Kind::Set => "SELECT id, uid, name, note, updated_at FROM project_sets WHERE uid IS NOT NULL",
    };
    let mut stmt = conn.prepare_cached(rows_sql)?;
    let rows: Vec<(i64, Node)> = stmt
        .query_map([], |r| {
            Ok((r.get(0)?, Node { uid: r.get(1)?, name: r.get(2)?, extra: r.get(3)?, updated_at: r.get(4)?, ..Node::default() }))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut live: BTreeMap<i64, Vec<SyncItem>> = BTreeMap::new();
    let sql = format!("SELECT {}, family_key, added_at FROM {}", kind.owner(), kind.items());
    for row in conn.prepare_cached(&sql)?.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
        let (id, key, at) = row?;
        live.entry(id).or_default().push(SyncItem { key, at, removed: false });
    }
    let mut removed: BTreeMap<String, Vec<SyncItem>> = BTreeMap::new();
    let mut stmt = conn.prepare_cached("SELECT owner_uid, family_key, removed_at FROM organizer_removed WHERE kind = ?1")?;
    for row in stmt.query_map([kind.name()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
        let (uid, key, at) = row?;
        removed.entry(uid).or_default().push(SyncItem { key, at, removed: true });
    }
    let mut nodes: Vec<Node> = rows
        .into_iter()
        .map(|(id, mut n)| {
            let mut items = live.remove(&id).unwrap_or_default();
            let keys: BTreeSet<String> = items.iter().map(|i| i.key.clone()).collect();
            items.extend(removed.remove(&n.uid).unwrap_or_default().into_iter().filter(|i| !keys.contains(&i.key)));
            items.sort_by(|a, b| a.key.cmp(&b.key));
            n.items = items;
            n
        })
        .collect();
    let mut stmt = conn.prepare_cached("SELECT entry_json FROM organizer_tombstones WHERE kind = ?1")?;
    for json in stmt.query_map([kind.name()], |r| r.get::<_, String>(0))? {
        let json = json?;
        // ponytail: a row that doesn't parse is skipped; only apply and record write these.
        let node = match kind {
            Kind::Collection => serde_json::from_str(&json).map(Node::from_collection),
            Kind::Set => serde_json::from_str(&json).map(Node::from_set),
        };
        nodes.extend(node.ok());
    }
    nodes.sort_by(|a, b| a.uid.cmp(&b.uid));
    Ok(nodes)
}

/// Gives new rows (no uid yet) their uid and `updated_at`, and new set items their `added_at`.
fn fill(conn: &Connection, now: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE collections SET uid = CASE builtin WHEN 'favorites' THEN 'favorites' ELSE lower(hex(randomblob(16))) END
         WHERE uid IS NULL AND builtin IS NOT 'recent'",
        [],
    )?;
    conn.execute("UPDATE collections SET updated_at = ?1 WHERE updated_at IS NULL AND uid IS NOT NULL", [now])?;
    conn.execute("UPDATE project_sets SET uid = lower(hex(randomblob(16))) WHERE uid IS NULL", [])?;
    conn.execute("UPDATE project_sets SET updated_at = ?1 WHERE updated_at IS NULL", [now])?;
    conn.execute("UPDATE project_set_items SET added_at = ?1 WHERE added_at IS NULL", [now])?;
    Ok(())
}

/// Stamps what changed since `before` (read in the same transaction) and, if anything did, counts one more
/// unsynced change. Returns whether anything changed.
pub(crate) fn record(conn: &Connection, before: &Local) -> rusqlite::Result<bool> {
    let now = now();
    fill(conn, &now)?;
    let after = Local::read(conn)?;
    let mut changed = false;
    for kind in [Kind::Collection, Kind::Set] {
        let live = |nodes: &[Node]| -> BTreeMap<String, Node> {
            nodes.iter().filter(|n| n.deleted_at.is_none()).map(|n| (n.uid.clone(), n.clone())).collect()
        };
        let (old, new) = (live(before.of(kind)), live(after.of(kind)));
        for (uid, was) in &old {
            if !new.contains_key(uid) {
                let mut dead = was.clone();
                dead.deleted_at = Some(later(&was.updated_at, &now));
                conn.execute(
                    "INSERT OR REPLACE INTO organizer_tombstones (kind, uid, updated_at, deleted_at, entry_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![kind.name(), uid, dead.updated_at, dead.deleted_at, dead.json(kind)],
                )?;
                conn.execute("DELETE FROM organizer_removed WHERE kind = ?1 AND owner_uid = ?2", params![kind.name(), uid])?;
                changed = true;
            }
        }
        for (uid, is) in &new {
            let was = old.get(uid).cloned().unwrap_or_default();
            changed |= stamp(conn, kind, &was, is, &now)?;
        }
    }
    if changed {
        conn.execute("UPDATE organizer_state SET dirty = dirty + 1", [])?;
    }
    Ok(changed)
}

/// One live row before and after a local edit (`was` is empty for a new row).
fn stamp(conn: &Connection, kind: Kind, was: &Node, is: &Node, now: &str) -> rusqlite::Result<bool> {
    let mut changed = false;
    if (&was.name, &was.extra) != (&is.name, &is.extra) {
        let sql = format!("UPDATE {} SET updated_at = ?2 WHERE uid = ?1", kind.table());
        conn.execute(&sql, params![is.uid, later(&was.updated_at, now)])?;
        changed = true;
    }
    let (old, new) = (was.live(), is.live());
    for (key, at) in old.iter().filter(|(k, _)| !new.contains_key(*k)) {
        conn.execute(
            "INSERT OR REPLACE INTO organizer_removed (kind, owner_uid, family_key, removed_at) VALUES (?1, ?2, ?3, ?4)",
            params![kind.name(), is.uid, key, later(at, now)],
        )?;
        changed = true;
    }
    for key in new.keys().filter(|k| !old.contains_key(*k)) {
        let removed_at = was.items.iter().find(|i| i.removed && i.key == *key).map_or("", |i| i.at.as_str());
        conn.execute("DELETE FROM organizer_removed WHERE kind = ?1 AND owner_uid = ?2 AND family_key = ?3", params![kind.name(), is.uid, key])?;
        let sql = format!(
            "UPDATE {} SET added_at = ?3 WHERE {} = (SELECT id FROM {} WHERE uid = ?1) AND family_key = ?2",
            kind.items(),
            kind.owner(),
            kind.table()
        );
        conn.execute(&sql, params![is.uid, key, later(removed_at, now)])?;
        changed = true;
    }
    Ok(changed)
}

/// Canonical timestamps, as the server stores them, and tombstones dropped a day before the server would drop
/// them, so a tombstone the server already pruned is never pushed back to it.
// ponytail: a day of margin; a server clock more than a day ahead re-sends such a tombstone once per round
// (the server answers it as a no-op) until this computer drops it too.
fn canonical(doc: &OrganizerDoc) -> Result<OrganizerDoc> {
    let now = Utc::now();
    let doc = core::normalize(doc, now).map_err(|e| EngineError::new("invalid", e))?;
    Ok(core::prune(&doc, &core::timestamp(now), KEEP_DAYS - 1))
}

/// Makes the stored rows match `target`; returns how many rows it wrote.
fn write(conn: &Connection, target: &OrganizerDoc) -> rusqlite::Result<usize> {
    let cut = core::loop_cuts(target);
    let collections: Vec<Node> = target.collections.iter().cloned().map(Node::from_collection).collect();
    let sets: Vec<Node> = target.sets.iter().cloned().map(Node::from_set).collect();
    Ok(write_kind(conn, Kind::Collection, &collections, &cut)? + write_kind(conn, Kind::Set, &sets, &cut)?)
}

fn write_kind(conn: &Connection, kind: Kind, nodes: &[Node], cut: &BTreeSet<String>) -> rusqlite::Result<usize> {
    let (t, k) = (kind.table(), kind.name());
    let mut n = 0;
    let (live, dead): (Vec<&Node>, Vec<&Node>) = nodes.iter().partition(|x| x.deleted_at.is_none());
    // Own fields; Favorites only takes `updated_at` (its name and place are fixed).
    let mut ids: BTreeMap<&str, i64> = BTreeMap::new();
    for x in &live {
        let id: Option<i64> = conn.query_row(&format!("SELECT id FROM {t} WHERE uid = ?1"), [&x.uid], |r| r.get(0)).optional()?;
        let note = x.extra.clone().unwrap_or_default();
        let id = match (id, kind) {
            (Some(id), _) if x.uid == FAVORITES => {
                n += conn.execute("UPDATE collections SET updated_at = ?2 WHERE id = ?1 AND updated_at IS NOT ?2", params![id, x.updated_at])?;
                id
            }
            (None, _) if x.uid == FAVORITES => continue,
            (Some(id), Kind::Collection) => {
                n += conn.execute(
                    "UPDATE collections SET name = ?2, updated_at = ?3 WHERE id = ?1 AND (name IS NOT ?2 OR updated_at IS NOT ?3)",
                    params![id, x.name, x.updated_at],
                )?;
                id
            }
            (Some(id), Kind::Set) => {
                n += conn.execute(
                    "UPDATE project_sets SET name = ?2, note = ?3, updated_at = ?4 WHERE id = ?1
                       AND (name IS NOT ?2 OR note IS NOT ?3 OR updated_at IS NOT ?4)",
                    params![id, x.name, note, x.updated_at],
                )?;
                id
            }
            (None, Kind::Collection) => {
                conn.execute("INSERT INTO collections (uid, name, updated_at) VALUES (?1, ?2, ?3)", params![x.uid, x.name, x.updated_at])?;
                n += 1;
                conn.last_insert_rowid()
            }
            (None, Kind::Set) => {
                conn.execute("INSERT INTO project_sets (uid, name, note, updated_at) VALUES (?1, ?2, ?3, ?4)", params![x.uid, x.name, note, x.updated_at])?;
                n += 1;
                conn.last_insert_rowid()
            }
        };
        ids.insert(&x.uid, id);
    }
    // Parents, once every row exists: a missing or deleted parent, or a cut parent loop, means top level.
    if kind == Kind::Collection {
        for x in live.iter().filter(|x| x.uid != FAVORITES) {
            let parent = x.extra.as_deref().filter(|_| !cut.contains(&x.uid)).and_then(|p| ids.get(p)).copied();
            n += conn.execute("UPDATE collections SET parent_id = ?2 WHERE id = ?1 AND parent_id IS NOT ?2", params![ids[x.uid.as_str()], parent])?;
        }
    }
    // Rows the document deleted (children were moved off them above, so the cascade takes only their families).
    let builtin = if kind == Kind::Collection { " AND builtin IS NULL" } else { "" };
    let rows: Vec<(i64, String)> = conn
        .prepare(&format!("SELECT id, uid FROM {t} WHERE uid IS NOT NULL{builtin}"))?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, uid) in rows.into_iter().filter(|(_, uid)| !ids.contains_key(uid.as_str())) {
        n += conn.execute(&format!("DELETE FROM {t} WHERE id = ?1"), [id])?;
        conn.execute("DELETE FROM organizer_removed WHERE kind = ?1 AND owner_uid = ?2", params![k, uid])?;
    }
    // Families.
    for x in &live {
        let Some(&id) = ids.get(x.uid.as_str()) else { continue };
        let want = x.live();
        let have: Vec<String> = conn
            .prepare_cached(&format!("SELECT family_key FROM {} WHERE {} = ?1", kind.items(), kind.owner()))?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for key in have.iter().filter(|h| !want.contains_key(h.as_str())) {
            n += conn.execute(&format!("DELETE FROM {} WHERE {} = ?1 AND family_key = ?2", kind.items(), kind.owner()), params![id, key])?;
        }
        let upsert = format!(
            "INSERT INTO {items} ({owner}, family_key, added_at) VALUES (?1, ?2, ?3)
             ON CONFLICT ({owner}, family_key) DO UPDATE SET added_at = excluded.added_at WHERE added_at IS NOT excluded.added_at",
            items = kind.items(),
            owner = kind.owner()
        );
        for (key, at) in &want {
            n += conn.execute(&upsert, params![id, key, at])?;
        }
        let gone: BTreeMap<&str, &str> = x.items.iter().filter(|i| i.removed).map(|i| (i.key.as_str(), i.at.as_str())).collect();
        let have: Vec<String> = conn
            .prepare_cached("SELECT family_key FROM organizer_removed WHERE kind = ?1 AND owner_uid = ?2")?
            .query_map(params![k, x.uid], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for key in have.iter().filter(|h| !gone.contains_key(h.as_str())) {
            n += conn.execute("DELETE FROM organizer_removed WHERE kind = ?1 AND owner_uid = ?2 AND family_key = ?3", params![k, x.uid, key])?;
        }
        for (key, at) in &gone {
            n += conn.execute(
                "INSERT INTO organizer_removed (kind, owner_uid, family_key, removed_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (kind, owner_uid, family_key) DO UPDATE SET removed_at = excluded.removed_at WHERE removed_at IS NOT excluded.removed_at",
                params![k, x.uid, key, at],
            )?;
        }
    }
    // Tombstones, exactly as the document has them.
    let have: Vec<(String, String)> = conn
        .prepare_cached("SELECT uid, entry_json FROM organizer_tombstones WHERE kind = ?1")?
        .query_map([k], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let want: BTreeMap<&str, &Node> = dead.iter().map(|x| (x.uid.as_str(), *x)).collect();
    for (uid, _) in have.iter().filter(|(u, _)| !want.contains_key(u.as_str())) {
        n += conn.execute("DELETE FROM organizer_tombstones WHERE kind = ?1 AND uid = ?2", params![k, uid])?;
    }
    let have: BTreeMap<String, String> = have.into_iter().collect();
    for (uid, x) in want {
        let json = x.json(kind);
        if have.get(uid) != Some(&json) {
            n += conn.execute(
                "INSERT OR REPLACE INTO organizer_tombstones (kind, uid, updated_at, deleted_at, entry_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![k, uid, x.updated_at, x.deleted_at, json],
            )?;
        }
    }
    Ok(n)
}

impl Engine {
    /// Runs one local edit of collections or project sets in a transaction and records it for sync.
    pub(crate) fn organizer_edit<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let before = Local::read(&tx)?;
        let out = f(&tx)?;
        record(&tx, &before)?;
        tx.commit()?;
        Ok(out)
    }

    /// The local organizer document, normalized like the server does. Remembers how many local changes it
    /// holds, for [`Engine::set_organizer_synced`].
    pub fn organizer_doc(&self) -> Result<OrganizerDoc> {
        let db = self.db();
        let doc = Local::read(&db)?.doc();
        let dirty: i64 = db.query_row("SELECT dirty FROM organizer_state", [], |r| r.get(0))?;
        *self.organizer_seen.lock().unwrap_or_else(|e| e.into_inner()) = dirty;
        canonical(&doc)
    }

    /// Makes local collections and project sets match `doc`, merged with what is stored here, so a change made
    /// while the document was on its way is kept. Local integer ids stay; Recent and a set's on/off and last use
    /// are never touched. Emits `library-changed` (reason `sync`) and returns true when anything changed.
    pub fn apply_organizer(&self, doc: &OrganizerDoc) -> Result<bool> {
        let changed = {
            let mut db = self.db();
            let tx = db.transaction()?;
            let local = canonical(&Local::read(&tx)?.doc())?;
            let target = canonical(&core::merge(&canonical(doc)?, &local))?;
            let n = if target == local { 0 } else { write(&tx, &target)? };
            tx.commit()?;
            n > 0
        };
        if changed {
            self.ui(UiEvent::LibraryChanged { reason: "sync" });
        }
        Ok(changed)
    }

    /// (rev of the last synced document, whether local changes wait to be pushed).
    pub fn organizer_sync_state(&self) -> Result<(i64, bool)> {
        let (rev, dirty): (i64, i64) = self.db().query_row("SELECT rev, dirty FROM organizer_state", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok((rev, dirty > 0))
    }

    /// Stores the server's rev and clears dirty, except for changes made after the last [`Engine::organizer_doc`]
    /// (they were not in what was pushed).
    pub fn set_organizer_synced(&self, rev: i64) -> Result<()> {
        let seen = std::mem::take(&mut *self.organizer_seen.lock().unwrap_or_else(|e| e.into_inner()));
        self.db().execute("UPDATE organizer_state SET rev = ?1, dirty = MAX(dirty - ?2, 0)", params![rev, seen])?;
        Ok(())
    }
}
