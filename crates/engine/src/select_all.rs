//! "Select all N results" in the Library: a Library query turned into every matching family, so the existing bulk
//! commands (`set_active`, `set_active_for`, `add_to_collection`, `set_pinned`) reach all of them, not one page.

use crate::{Engine, FamilyQuery, Result};
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
}
