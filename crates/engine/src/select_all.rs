//! "Select all N results" in the Library: a Library query turned into every matching family, so the existing bulk
//! commands (`set_active`, `set_active_for`, `add_to_collection`, `set_pinned`) reach all of them, not one page.

use crate::library::clean_tags;
use crate::{Engine, FamilyQuery, Result};
use rusqlite::params;
use serde::Serialize;

/// Every family a Library query matches, in list order.
#[derive(Debug, Clone, Serialize)]
pub struct QueryTargets {
    pub keys: Vec<String>,
    /// Their fonts that can be turned on and off (system fonts are always on and left out).
    pub hashes: Vec<String>,
}

impl Engine {
    // ponytail: runs the full list query and keeps only keys and hashes; fine while `family_page` does the same.
    pub fn query_targets(&self, q: &FamilyQuery) -> Result<QueryTargets> {
        let fams = self.list_families(q)?;
        let hashes = fams.iter().flat_map(|f| f.faces.iter().filter(|x| !x.system).map(|x| x.hash.clone())).collect();
        Ok(QueryTargets { keys: fams.into_iter().map(|f| f.key).collect(), hashes })
    }

    /// Adds `add` to and removes `remove` from every family in `keys`, in one transaction (a tag in both is
    /// removed). Tag names follow the `set_tags` rule. Returns how many families' tags changed.
    pub fn edit_tags(&self, keys: &[String], add: &[String], remove: &[String]) -> Result<usize> {
        let remove = clean_tags(remove);
        let add: Vec<String> = clean_tags(add).into_iter().filter(|t| !remove.contains(t)).collect();
        let mut db = self.db();
        let tx = db.transaction()?;
        let mut changed = 0;
        for key in keys {
            let mut n = 0;
            for t in &remove {
                n += tx.execute("DELETE FROM family_tags WHERE family_key = ?1 AND tag = ?2", params![key, t])?;
            }
            for t in &add {
                n += tx.execute("INSERT OR IGNORE INTO family_tags (family_key, tag) VALUES (?1, ?2)", params![key, t])?;
            }
            changed += usize::from(n > 0);
        }
        tx.commit()?;
        Ok(changed)
    }
}
