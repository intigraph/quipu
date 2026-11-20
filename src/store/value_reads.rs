//! Current facts in ONE graph whose object is a given value (aegis-550z).
//!
//! Fluent binding and log reads ask which facts point AT one node. Loading the
//! whole graph to answer that held the write lock for a full reserialize per
//! append; `idx_active_vge (v, g, e)` answers it with one probe.
use rusqlite::params;

use super::Store;
use crate::{Fact, Result, Value};

impl Store {
    /// Current asserted facts in graph `g` whose object equals `value`.
    ///
    /// Same visibility and scope as [`Store::current_facts_in_graph`]: the
    /// graph's OWN facts, not a composed view.
    pub fn current_facts_with_value_in_graph(&self, value: &Value, g: i64) -> Result<Vec<Fact>> {
        let mut stmt = self.conn.prepare(
            "SELECT e, a, v, tx, valid_from, valid_to, op FROM facts \
             WHERE v = ?1 AND g = ?2 AND op = 1 AND valid_to IS NULL \
             ORDER BY e, a, tx",
        )?;
        Self::collect_visible_facts(&mut stmt, params![value.to_bytes(), g])
    }
}
