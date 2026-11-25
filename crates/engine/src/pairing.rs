//! Playground helpers: pairing suggestions from the stored metrics and named variable-axis presets.

use crate::{not_found, store, Engine, EngineError, Family, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use typeshelf_core::Metrics;

/// Named axis values (`tag` → value) saved for one family.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AxisPreset {
    pub name: String,
    pub values: BTreeMap<String, f32>,
}

/// The stored x-height of face (`hash`, `idx`) as a fraction of the em; 0 when unknown.
pub(crate) fn x_height(conn: &Connection, hash: &str, idx: i64) -> rusqlite::Result<f32> {
    let json: String = conn
        .prepare_cached("SELECT metrics_json FROM faces WHERE hash = ?1 AND idx = ?2")?
        .query_row(params![hash, idx], |r| r.get(0))?;
    Ok(serde_json::from_str::<Metrics>(&json).unwrap_or_default().x_height)
}

fn presets_key(family_key: &str) -> String {
    format!("axis_presets:{family_key}")
}

impl Engine {
    /// Up to 8 families that contrast with `key`: a known category different from its own, nearest
    /// x-height first (families without one come last).
    // ponytail: category + x-height only; weigh in contrast/width if suggestions feel off.
    pub fn pairing_suggestions(&self, key: &str) -> Result<Vec<Family>> {
        let shapes = self.family_shapes()?;
        let me = &shapes.get(key).ok_or_else(|| not_found("family"))?.metrics;
        let gap = |m: &Metrics| if me.x_height > 0.0 && m.x_height > 0.0 { (me.x_height - m.x_height).abs() } else { f32::INFINITY };
        let mut ranked: Vec<(f32, &String)> = shapes
            .iter()
            .filter(|(k, s)| *k != key && s.metrics.category.is_some() && s.metrics.category != me.category)
            .map(|(k, s)| (gap(&s.metrics), k))
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        self.families_in_order(ranked.into_iter().take(8).map(|(_, k)| k.clone()).collect())
    }

    pub fn axis_presets(&self, family_key: &str) -> Result<Vec<AxisPreset>> {
        Ok(store::get(&self.db(), &presets_key(family_key))?.unwrap_or_default())
    }

    /// Saves (or replaces, by name) a preset; returns the family's presets.
    pub fn save_axis_preset(&self, family_key: &str, name: &str, values: &BTreeMap<String, f32>) -> Result<Vec<AxisPreset>> {
        let name = name.trim();
        if name.is_empty() {
            return Err(EngineError::new("invalid", "name is empty"));
        }
        let db = self.db();
        let key = presets_key(family_key);
        let mut list: Vec<AxisPreset> = store::get(&db, &key)?.unwrap_or_default();
        let preset = AxisPreset { name: name.into(), values: values.clone() };
        match list.iter_mut().find(|p| p.name == name) {
            Some(p) => *p = preset,
            None => list.push(preset),
        }
        store::set(&db, &key, &list)?;
        Ok(list)
    }

    pub fn delete_axis_preset(&self, family_key: &str, name: &str) -> Result<Vec<AxisPreset>> {
        let db = self.db();
        let key = presets_key(family_key);
        let mut list: Vec<AxisPreset> = store::get(&db, &key)?.unwrap_or_default();
        list.retain(|p| p.name != name);
        if list.is_empty() {
            store::remove(&db, &key)?;
        } else {
            store::set(&db, &key, &list)?;
        }
        Ok(list)
    }
}
