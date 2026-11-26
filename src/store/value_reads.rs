//! Current facts in ONE graph whose object is a given value (aegis-550z).
//!
//! Fluent binding and log reads ask which facts point AT one node. Loading the
//! whole graph to answer that held the write lock for a full reserialize per
//! append; `idx_active_vge (v, g, e)` answers it with one probe.
use rusqlite::params;

use super::Store;
use crate::{Fact, Result, Value};

// ORDER BY otherwise tempts SQLite into a graph-only scan through idx_current_gea.
// Select the existing value-leading index before sorting the matching rows.
const CURRENT_VALUE_FACTS: &str = "SELECT e, a, v, tx, valid_from, valid_to, op FROM facts INDEXED BY idx_active_vge \
     WHERE v = ?1 AND g = ?2 AND op = 1 AND valid_to IS NULL \
     ORDER BY e, a, tx";

impl Store {
    /// Current asserted facts in graph `g` whose object equals `value`.
    ///
    /// Same visibility and scope as [`Store::current_facts_in_graph`]: the
    /// graph's OWN facts, not a composed view.
    pub fn current_facts_with_value_in_graph(&self, value: &Value, g: i64) -> Result<Vec<Fact>> {
        let mut stmt = self.conn.prepare(CURRENT_VALUE_FACTS)?;
        Self::collect_visible_facts(&mut stmt, params![value.to_bytes(), g])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_value_probe_uses_both_value_and_graph_index_keys() {
        let store = Store::open_in_memory().unwrap();
        let mut statement = store
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {CURRENT_VALUE_FACTS}"))
            .unwrap();
        let plan: Vec<String> = statement
            .query_map(params![Value::Ref(42).to_bytes(), 0], |row| row.get(3))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert!(
            plan.iter()
                .any(|step| step.contains("idx_active_vge") && step.contains("v=? AND g=?")),
            "ordered value probe must seek both keys, plan={plan:?}"
        );
    }
}
