//! Usage statistics and badges, kept on this computer only. Every font the user turns on or off is written to
//! `usage_log` by the activation code, in the same savepoint as its ledger row. An Adobe document turning a
//! font on counts (it comes in as the user: the font is used); turn-offs the user did not ask for (quit,
//! folder removed, Adobe document closed, a file gone, a temporary font expiring) and auto-activation of
//! arriving files do not. Admin-pushed changes are not the user's use and are not recorded.
//! Rows older than `KEEP_DAYS` are deleted at every check, so the log stays bounded.
// ponytail: a queued (background) registration that fails afterwards keeps its "on" row, so it still counts
// as a day of use; remove the row in `register_queued`'s error branch if that ever matters.

use crate::{store, Engine, Result, UiEvent};
use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::TryLockError;
use typeshelf_protocol::Source;

/// Rows older than this are deleted: the longest look-back is "not used for a year" (365 days).
const KEEP_DAYS: i64 = 400;
const COLLECTOR_FONTS: u32 = 1000;
const QUIET_FONTS: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsagePeriod {
    Month,
    Year,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyUse {
    pub key: String,
    pub family: String,
    /// Days (UTC) on which a font of the family was turned on.
    pub days: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnusedFamily {
    pub key: String,
    pub family: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStats {
    pub period: UsagePeriod,
    /// Start of the period (UTC).
    pub since: String,
    /// Top 10, most days first.
    pub most_used: Vec<FamilyUse>,
    /// Families not turned on in the last 12 months (or since `recording_since`, if later), not on now and in
    /// the library for 30 days; empty while `never_used_note` says why.
    pub never_used: Vec<UnusedFamily>,
    pub never_used_note: Option<String>,
    /// Font files in the user's folders (system fonts not counted).
    pub total_fonts: u32,
    pub added_in_period: u32,
    /// When recording started (this version's first run).
    pub recording_since: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Badge {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub earned_at: Option<String>,
}

const BADGES: [(&str, &str, &str); 3] = [
    ("collector", "Collector", "1,000 fonts in your library."),
    ("archaeologist", "Archaeologist", "Turned on a font nobody had used for a year."),
    ("quiet", "Quiet", "Turned every font off at once (at least 10)."),
];

fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Records one font turned on or off, when the user caused it. Never fails the activation.
pub(crate) fn record(conn: &Connection, hash: &str, active: bool, source: Source) {
    if !matches!(source, Source::User | Source::ProjectSet) {
        return;
    }
    let _ = conn
        .prepare_cached(
            "INSERT INTO usage_log (family_key, hash, active, source, at)
             SELECT family_key, ?1, ?2, ?3, ?4 FROM faces WHERE hash = ?1 ORDER BY idx LIMIT 1",
        )
        .and_then(|mut s| s.execute(params![hash, active, crate::activation::source_str(source), store::now()]));
}

/// The user's font files (not the OS font folders) and when each first arrived.
const USER_FILES: &str = "SELECT fi.hash, MIN(fi.added_at) AS added FROM files fi JOIN folders fo ON fo.id = fi.folder_id
     WHERE fo.managed IS NOT 'system' GROUP BY fi.hash";

fn library_fonts(conn: &Connection) -> rusqlite::Result<u32> {
    conn.query_row(&format!("SELECT COUNT(*) FROM ({USER_FILES})"), [], |r| r.get(0))
}

/// Awards badges for what happened since the last check; returns the new ones.
fn award(conn: &Connection, now: DateTime<Utc>) -> rusqlite::Result<Vec<Badge>> {
    conn.execute("DELETE FROM usage_log WHERE at < ?1", [ts(now - Duration::days(KEEP_DAYS))])?;
    let seen: i64 = store::get(conn, "usage_seen")?.unwrap_or(0);
    let last: i64 = conn.query_row("SELECT COALESCE(MAX(id), 0) FROM usage_log", [], |r| r.get(0))?;
    let has = |id: &str| conn.query_row("SELECT 1 FROM badges WHERE id = ?1", [id], |_| Ok(())).optional().map(|r| r.is_some());
    let mut new = Vec::new();

    if !has("collector")? && library_fonts(conn)? >= COLLECTOR_FONTS {
        new.push("collector");
    }
    if !has("archaeologist")? && last > seen {
        let year_ago = ts(now - Duration::days(365));
        let since: Option<String> = store::get(conn, "usage_since")?;
        // Only once a year has been recorded can "unused for a year" be told from "never seen".
        if since.is_some_and(|s| s <= year_ago) {
            let found = conn
                .query_row(
                    "SELECT 1 FROM usage_log u WHERE u.id > ?1 AND u.active = 1 AND u.at >= ?2
                       AND NOT EXISTS (SELECT 1 FROM usage_log p WHERE p.family_key = u.family_key AND p.active = 1
                                       AND p.id < u.id AND p.at >= ?2)
                       AND (SELECT MIN(fi.added_at) FROM faces fa JOIN files fi ON fi.hash = fa.hash JOIN folders fo ON fo.id = fi.folder_id
                            WHERE fa.family_key = u.family_key AND fo.managed IS NOT 'system') <= ?2
                     LIMIT 1",
                    params![seen, year_ago],
                    |_| Ok(()),
                )
                .optional()?;
            if found.is_some() {
                new.push("archaeologist");
            }
        }
    }
    if !has("quiet")? && last > seen {
        // Everything since the last check is one user action: checks run after each one.
        let off: u32 = conn.query_row("SELECT COUNT(*) FROM usage_log WHERE id > ?1 AND active = 0", [seen], |r| r.get(0))?;
        let still_on: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM activations WHERE source != 'admin')", [], |r| r.get(0))?;
        if off >= QUIET_FONTS && !still_on {
            new.push("quiet");
        }
    }
    if last != seen {
        store::set(conn, "usage_seen", &last)?;
    }
    let at = ts(now);
    let mut out = Vec::new();
    for id in new {
        if conn.execute("INSERT OR IGNORE INTO badges (id, earned_at) VALUES (?1, ?2)", params![id, at])? > 0 {
            let (id, name, description) = BADGES.into_iter().find(|b| b.0 == id).expect("known badge");
            out.push(Badge { id, name, description, earned_at: Some(at.clone()) });
        }
    }
    Ok(out)
}

impl Engine {
    /// After every library change: awards new badges and tells the UI. Skipped (until the next change)
    /// when the database is busy, so it can never block or deadlock a caller that still holds it.
    pub(crate) fn check_badges(&self) {
        let db = match self.db.try_lock() {
            Ok(db) => db,
            Err(TryLockError::Poisoned(e)) => e.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        let new = award(&db, Utc::now()).unwrap_or_default();
        drop(db);
        for b in new {
            self.ui(UiEvent::BadgeEarned(b));
        }
    }

    /// All badges, earned or not.
    pub fn badges(&self, now: DateTime<Utc>) -> Result<Vec<Badge>> {
        let (new, earned) = {
            let db = self.db();
            let new = award(&db, now)?;
            let mut stmt = db.prepare("SELECT id, earned_at FROM badges")?;
            let earned: Vec<(String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
            (new, earned)
        };
        for b in new {
            self.ui(UiEvent::BadgeEarned(b));
        }
        Ok(BADGES
            .into_iter()
            .map(|(id, name, description)| Badge {
                id,
                name,
                description,
                earned_at: earned.iter().find(|e| e.0 == id).map(|e| e.1.clone()),
            })
            .collect())
    }

    pub fn usage_stats(&self, period: UsagePeriod, now: DateTime<Utc>) -> Result<UsageStats> {
        let start = match period {
            UsagePeriod::Month => Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0),
            UsagePeriod::Year => Utc.with_ymd_and_hms(now.year(), 1, 1, 0, 0, 0),
        }
        .single()
        .expect("first of a month is a valid date");
        let (since, until) = (ts(start), ts(now));
        let db = self.db();

        // ponytail: days are UTC days; use the local offset if people near midnight notice.
        let mut stmt = db.prepare(
            "SELECT u.family_key, COUNT(DISTINCT substr(u.at, 1, 10)) AS days,
                    (SELECT MIN(family) FROM faces WHERE family_key = u.family_key) AS family
             FROM usage_log u WHERE u.active = 1 AND u.at >= ?1 AND u.at <= ?2
             GROUP BY u.family_key HAVING family IS NOT NULL ORDER BY days DESC, family LIMIT 10",
        )?;
        let most_used = stmt
            .query_map([&since, &until], |r| Ok(FamilyUse { key: r.get(0)?, days: r.get(1)?, family: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?;

        // The library's age is the arrival of its oldest font file in the user's folders. Both it and the
        // recording must be 30 days old, and a family only counts once its own first file is.
        let oldest: Option<String> = db.query_row(&format!("SELECT MIN(added) FROM ({USER_FILES})"), [], |r| r.get(0))?;
        let recording_since: Option<String> = store::get(&db, "usage_since")?;
        let month_ago = ts(now - Duration::days(30));
        let (never_used, never_used_note) = match oldest {
            None => (Vec::new(), Some("Add a folder of fonts first.".into())),
            Some(o) if o > month_ago => {
                (Vec::new(), Some("Your library is less than 30 days old, so it is too early to tell which fonts go unused.".into()))
            }
            _ if recording_since.as_ref().is_none_or(|s| *s > month_ago) => {
                (Vec::new(), Some("Typeshelf started counting less than 30 days ago, so it is too early to tell which fonts go unused.".into()))
            }
            _ => {
                let mut stmt = db.prepare(&format!(
                    "SELECT fa.family_key, MIN(fa.family) FROM ({USER_FILES}) uf JOIN faces fa ON fa.hash = uf.hash
                     WHERE NOT EXISTS (SELECT 1 FROM usage_log u WHERE u.family_key = fa.family_key AND u.active = 1 AND u.at >= ?1)
                       AND NOT EXISTS (SELECT 1 FROM activations a JOIN faces g ON g.hash = a.hash WHERE g.family_key = fa.family_key)
                     GROUP BY fa.family_key HAVING MIN(uf.added) <= ?2 ORDER BY MIN(fa.family)"
                ))?;
                let rows = stmt
                    .query_map([ts(now - Duration::days(365)), month_ago], |r| Ok(UnusedFamily { key: r.get(0)?, family: r.get(1)? }))?
                    .collect::<rusqlite::Result<_>>()?;
                (rows, None)
            }
        };

        let total_fonts = library_fonts(&db)?;
        let added_in_period = db.query_row(&format!("SELECT COUNT(*) FROM ({USER_FILES}) WHERE added >= ?1"), [&since], |r| r.get(0))?;
        Ok(UsageStats {
            period,
            since,
            most_used,
            never_used,
            never_used_note,
            total_fonts,
            added_in_period,
            recording_since,
        })
    }
}
