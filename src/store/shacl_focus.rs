//! Which value-set verdicts a write owns (aegis-2i6).
//!
//! A write owns a violation on `(focus, path)` only when it adds a value that
//! was not already current on that path, read forward or inversely, or when
//! it newly makes the focus a target (a new type, or a new edge on a
//! `sh:targetSubjectsOf`/`sh:targetObjectsOf` predicate). A count that was
//! already over its bound before the write is not the write's to judge:
//! restating a label, or adding `modifies` to a module that already carries
//! two labels, changes no count. Refusing those froze every producer linking
//! to such a node (`QuipuIngestFailing`, 2026-10-21). A write that adds a value
//! to an over-bound path still makes it worse and is still refused.

use std::collections::BTreeSet;

use super::{Datum, Store};
use crate::Result;
use crate::shacl::write_paths::CardinalityPlan;

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

#[derive(Default)]
pub(super) struct ValueSetOwnership {
    /// Focuses this write newly targets: every path is theirs.
    pub(super) whole: BTreeSet<String>,
    /// `(focus, path)` pairs gaining a new value; inverse paths as `^iri`.
    paths: BTreeSet<(String, String)>,
    /// Focuses gaining any new value. Comparisons (`sh:disjoint`,
    /// `sh:lessThan`, ...) report on one path but read a second one too.
    gained: BTreeSet<String>,
}

impl ValueSetOwnership {
    pub(super) fn owns(&self, component: &str, focus: &str, path: Option<&str>) -> bool {
        if self.whole.contains(focus) {
            return true;
        }
        if !matches!(
            component,
            "MaxCountConstraintComponent" | "UniqueLangConstraintComponent"
        ) {
            return self.gained.contains(focus);
        }
        path.is_some_and(|path| {
            let path: String = path
                .chars()
                .filter(|c| !matches!(c, '<' | '>' | '(' | ')') && !c.is_whitespace())
                .collect();
            self.paths.contains(&(focus.to_owned(), path))
        })
    }
}

impl Store {
    pub(super) fn value_set_ownership(
        &self,
        datums: &[Datum],
        graph: i64,
        tx_id: i64,
        plan: &CardinalityPlan,
    ) -> Result<ValueSetOwnership> {
        let mut owned = ValueSetOwnership::default();
        for datum in datums {
            if datum.op != crate::types::Op::Assert || !self.newly_current(datum, graph, tx_id)? {
                continue;
            }
            let subject = self.resolve(datum.entity)?;
            let predicate = self.resolve(datum.attribute)?;
            if predicate == RDF_TYPE || plan.subject_target_predicates.contains(&predicate) {
                owned.whole.insert(subject.clone());
            }
            if let crate::Value::Ref(object) = datum.value {
                let object = self.resolve(object)?;
                if plan.object_target_predicates.contains(&predicate) {
                    owned.whole.insert(object.clone());
                }
                owned.gained.insert(object.clone());
                owned.paths.insert((object, format!("^{predicate}")));
            }
            owned.gained.insert(subject.clone());
            owned.paths.insert((subject, predicate));
        }
        Ok(owned)
    }

    /// Whether this transaction made the fact current. Any doubt counts as
    /// new, which keeps the write's verdict owned.
    fn newly_current(&self, datum: &Datum, graph: i64, tx_id: i64) -> Result<bool> {
        let before: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM main.facts WHERE e = ?1 AND a = ?2 AND v = ?3 AND g = ?4
                 AND op = 1 AND valid_to IS NULL AND tx < ?5 LIMIT 1",
                rusqlite::params![
                    datum.entity,
                    datum.attribute,
                    datum.value.to_bytes(),
                    graph,
                    tx_id
                ],
                |row| row.get(0),
            )
            .ok();
        Ok(before.is_none())
    }
}
