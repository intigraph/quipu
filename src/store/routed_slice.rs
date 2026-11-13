//! Copy one producer's current facts between files by canonical terms (per-graph
//! pilot, parity step). File-local term ids never cross files: IRIs are resolved
//! on export and re-interned on import.
//!
//! The exported set is exactly what `cut_over_routed_source` retracts from ROOT
//! (the same source-retraction plan), so "copied" and "retracted" cannot drift.
//! Import writes under the SAME `snapshot:<key>` tag, so the producer's next
//! `replace_snapshot` in the owner replaces the copied facts, not adds to them.

use super::{Datum, Store};
use crate::types::{Op, Value};
use crate::{Error, Result};

/// One current fact in canonical, file-independent form.
#[derive(Clone, Debug, PartialEq)]
pub struct SliceFact {
    pub subject: String,
    pub predicate: String,
    /// `Ok(iri)` for an IRI object, `Err(literal)` for any literal value.
    pub object: std::result::Result<String, Value>,
    pub valid_from: String,
}

impl SliceFact {
    /// A stable text form for multiset comparison.
    #[must_use]
    pub fn canonical(&self) -> String {
        let object = match &self.object {
            Ok(iri) => format!("<{iri}>"),
            Err(value) => format!("{value:?}"),
        };
        format!(
            "<{}> <{}> {object} @{}",
            self.subject, self.predicate, self.valid_from
        )
    }
}

fn refused(message: impl std::fmt::Display) -> Error {
    Error::PolicyDenied(format!("graph owner: {message}"))
}

/// Sorted canonical forms: equal vectors mean equal fact multisets.
#[must_use]
pub fn slice_multiset(facts: &[SliceFact]) -> Vec<String> {
    let mut out: Vec<String> = facts.iter().map(SliceFact::canonical).collect();
    out.sort();
    out
}

impl Store {
    fn graph_id_for(&self, graph: &str) -> Result<i64> {
        if graph == crate::schema::ROOT_GRAPH_IRI {
            return Ok(0);
        }
        let id = self
            .lookup(graph)?
            .ok_or_else(|| refused("slice graph is not registered here"))?;
        if self.graph_class(id)?.as_deref() != Some("committed") {
            return Err(refused("slice graph must be committed"));
        }
        Ok(id)
    }

    /// The current facts `source_tag` owns in `graph`, in canonical form.
    pub fn export_routed_slice(&self, source_tag: &str, graph: &str) -> Result<Vec<SliceFact>> {
        let g = self.graph_id_for(graph)?;
        self.plan_source_retraction(source_tag, g)?
            .into_iter()
            .map(|datum| {
                Ok(SliceFact {
                    subject: self.resolve(datum.entity)?,
                    predicate: self.resolve(datum.attribute)?,
                    object: match datum.value {
                        Value::Ref(id) => Ok(self.resolve(id)?),
                        literal => Err(literal),
                    },
                    valid_from: datum.valid_from,
                })
            })
            .collect()
    }

    /// Assert `slice` into `graph` under `source_tag`, preserving valid time.
    /// Refuses when that tag already owns current facts in `graph`, so a
    /// repeated import cannot double the slice. Returns the transaction id.
    pub(crate) fn import_routed_slice(
        &mut self,
        slice: &[SliceFact],
        source_tag: &str,
        graph: &str,
        timestamp: &str,
    ) -> Result<Option<i64>> {
        if source_tag
            .strip_prefix("snapshot:")
            .is_none_or(|key| key.trim().is_empty())
        {
            return Err(refused("a producer source tag is `snapshot:<key>`"));
        }
        let g = self.graph_id_for(graph)?;
        if g == 0 {
            return Err(refused("a slice is imported into a routed graph, not ROOT"));
        }
        if !self.plan_source_retraction(source_tag, g)?.is_empty() {
            return Err(refused("the tag already owns current facts in this graph"));
        }
        if slice.is_empty() {
            return Ok(None);
        }
        let datums = slice
            .iter()
            .map(|fact| {
                Ok(Datum {
                    entity: self.intern(&fact.subject)?,
                    attribute: self.intern(&fact.predicate)?,
                    value: match &fact.object {
                        Ok(iri) => Value::Ref(self.intern(iri)?),
                        Err(literal) => literal.clone(),
                    },
                    valid_from: fact.valid_from.clone(),
                    valid_to: None,
                    op: Op::Assert,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.transact_to_graph_scoped(
            &datums,
            timestamp,
            None,
            Some(source_tag),
            g,
            Some(source_tag),
        )
        .map(Some)
    }
}

#[cfg(test)]
#[path = "routed_slice_tests.rs"]
mod tests;
