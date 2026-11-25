//! Temporary activation: fonts turned on "for 2 hours" or "until I restart Typeshelf" turn themselves off.
//! The expiry per font hash lives in the settings store under `temporary` (`"restart"` or an RFC 3339 time);
//! every path that turns a font on or off drops its record ([`forget`]), so only the temporary turn-on counts.

use crate::{store, ActivationResult, Engine, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use typeshelf_protocol::Source;

const KEY: &str = "temporary";
const RESTART: &str = "restart";

/// How long [`Engine::set_active_for`] keeps fonts on: `"restart"` or `{ "minutes": 120 }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Until {
    Restart,
    Minutes(u32),
}

/// Font hash → `"restart"` or the RFC 3339 time it turns off.
type Records = BTreeMap<String, String>;

fn save(conn: &Connection, records: &Records) -> rusqlite::Result<()> {
    if records.is_empty() { store::remove(conn, KEY) } else { store::set(conn, KEY, records) }
}

/// Drops the records of fonts that were just turned on or off another way.
pub(crate) fn forget(conn: &Connection, hashes: &[String]) -> rusqlite::Result<()> {
    let Some(mut records) = store::get::<Records>(conn, KEY)? else { return Ok(()) };
    let before = records.len();
    for h in hashes {
        records.remove(h);
    }
    if records.len() != before {
        save(conn, &records)?;
    }
    Ok(())
}

/// An active project set includes the font's family.
fn needed_by_a_set(conn: &Connection, hash: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT 1 FROM faces f JOIN project_set_items i ON i.family_key = f.family_key JOIN project_sets s ON s.id = i.set_id
         WHERE f.hash = ?1 AND s.is_on = 1 LIMIT 1",
        [hash],
        |_| Ok(()),
    )
    .optional()
    .map(|r| r.is_some())
}

impl Engine {
    /// Turns fonts on like `set_active` and records when each one turns off again. A font that is already on
    /// for good stays on for good; one that is already on temporarily gets the new time.
    pub fn set_active_for(&self, hashes: &[String], until: Until) -> Result<Vec<ActivationResult>> {
        let permanent: HashSet<String> = {
            let db = self.db();
            let records: Records = store::get(&db, KEY)?.unwrap_or_default();
            self.ledger(&db)?.into_iter().map(|r| r.hash).filter(|h| !records.contains_key(h)).collect()
        };
        let results = self.set_active(hashes, true)?;
        let value = match until {
            Until::Restart => RESTART.to_string(),
            Until::Minutes(m) => {
                (Utc::now() + chrono::Duration::minutes(m.into())).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            }
        };
        {
            let db = self.db();
            let mut records: Records = store::get(&db, KEY)?.unwrap_or_default();
            // On in the ledger now: not a system font (always on) or a background registration that already failed.
            let on: HashSet<String> = self.ledger(&db)?.into_iter().map(|r| r.hash).collect();
            for r in results.iter().filter(|r| r.ok && !permanent.contains(&r.hash) && on.contains(&r.hash)) {
                records.insert(r.hash.clone(), value.clone());
            }
            save(&db, &records)?;
        }
        self.library_changed("activation");
        Ok(results)
    }

    /// Turns off every temporary font whose time has passed (`startup`: also the "until restart" ones) and
    /// returns how many. A font that is on for another reason (an admin, a library push, an active project
    /// set) only loses its record. The due records are taken in one database step, so two sweeps running at
    /// once never pick the same font.
    pub fn expire_temporary(&self, now: DateTime<Utc>, startup: bool) -> Result<usize> {
        let due: Records = {
            let db = self.db();
            let Some(mut records) = store::get::<Records>(&db, KEY)? else { return Ok(0) };
            let sources: HashMap<String, Source> = self.ledger(&db)?.into_iter().map(|r| (r.hash, r.source)).collect();
            let mut due = Records::new();
            for (hash, until) in std::mem::take(&mut records) {
                let expired = if until == RESTART {
                    startup
                } else {
                    DateTime::parse_from_rfc3339(&until).map_or(true, |t| t <= now)
                };
                if !expired {
                    records.insert(hash, until);
                } else if sources.get(&hash) == Some(&Source::User) && !needed_by_a_set(&db, &hash)? {
                    due.insert(hash, until);
                }
            }
            save(&db, &records)?;
            due
        };
        if due.is_empty() {
            return Ok(0);
        }
        // ponytail: the normal user turn-off; a font the user turns on again in the instant between the step
        // above and this call still goes off. Recheck under the lock if that ever matters.
        let hashes: Vec<String> = due.keys().cloned().collect();
        let results = self.set_active_now_as(&hashes, false, false)?;
        // The OS refused: keep the record, so the next sweep tries again instead of leaving the font on for good.
        let failed: Vec<&String> = results.iter().filter(|r| !r.ok).map(|r| &r.hash).collect();
        if !failed.is_empty() {
            let db = self.db();
            let mut records: Records = store::get(&db, KEY)?.unwrap_or_default();
            for h in failed {
                if let Some(until) = due.get(h) {
                    records.insert(h.clone(), until.clone());
                }
            }
            save(&db, &records)?;
        }
        Ok(results.iter().filter(|r| r.ok).count())
    }
}
