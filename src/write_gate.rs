//! The pre-transaction write gate shared by SPARQL `/update` and proposal
//! migrations (aegis-1hfyk5, aegis-esd).
//!
//! The gate judges a planned change set, expressed per destination graph as
//! N-Triples, before any term is interned or any transaction begins, so a
//! refusal stores nothing. Same order and semantics as `/knot`: the closed
//! vocabulary first, then each destination graph's asserted triples against
//! the given shapes with that graph's store context. The reject half blocks;
//! the emit half only queues `shacl.violation` events. A retraction is judged
//! only where it can break a node: a subject losing a value on a required path
//! must not end up with a violation its pre-state did not have (aegis-g51c
//! C12).
//!
//! The gate never records its own refusal. A caller holding a savepoint
//! records it after rolling back (see [`Refusal::record`]), so the evidence
//! survives the rollback that discards everything else.

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::store::Store;

/// Planned triples per destination graph IRI (`""` = ROOT), as N-Triples.
pub type TriplesByGraph = BTreeMap<String, String>;

/// Why the gate refused a change set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// Which gate refused: `vocabulary` or `shacl`.
    pub gate: &'static str,
    /// The refusal text, ending "No facts were written".
    pub reason: String,
    /// The graph whose triples were refused.
    pub graph: i64,
}

impl Refusal {
    /// Record the refusal on the audit spine and turn it into the error the
    /// caller returns.
    pub fn record(self, store: &Store, actor: Option<&str>, source: &str) -> Error {
        if self.gate == "shacl" {
            let now = crate::time::now_iso();
            store.record_gate_refusal("shacl", &self.reason, self.graph, actor, Some(source), &now);
        }
        Error::InvalidValue(self.reason)
    }
}

/// Judge `asserted` and `removed` against the vocabulary and `shapes`.
///
/// `Ok(None)` means the change may be transacted. A vocabulary refusal comes
/// back as `Err` (it is not a SHACL refusal and was never recorded as one).
pub fn check(
    store: &Store,
    shapes: Option<&str>,
    asserted: &TriplesByGraph,
    removed: &TriplesByGraph,
) -> Result<Option<Refusal>> {
    if !asserted.is_empty() {
        let all: String = asserted.values().map(String::as_str).collect();
        crate::vocabulary::enforce_turtle(store, &all)?;
        if let Some(refusal) = check_shapes(store, shapes, asserted)? {
            return Ok(Some(refusal));
        }
    }
    if removed.is_empty() {
        return Ok(None);
    }
    check_retractions(store, shapes, asserted, removed)
}

/// The graph id a destination IRI resolves to; a graph this write creates has
/// no facts of its own yet, so ROOT is its whole context.
#[cfg(feature = "shacl")]
fn graph_id(store: &Store, iri: &str) -> Result<i64> {
    Ok(if iri.is_empty() {
        crate::schema::ROOT_GRAPH
    } else {
        store.lookup(iri)?.unwrap_or(crate::schema::ROOT_GRAPH)
    })
}

/// The first three results of a refused validation, for the refusal text.
#[cfg(feature = "shacl")]
pub fn refusal_detail(feedback: &crate::shacl::ValidationFeedback) -> String {
    feedback
        .results
        .iter()
        .take(3)
        .map(|r| {
            format!(
                "{}: {} [{}] ({})",
                r.severity,
                r.message.as_deref().unwrap_or("no message"),
                r.source_shape.as_deref().unwrap_or("?"),
                r.focus_node
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(feature = "shacl")]
fn check_shapes(
    store: &Store,
    shapes: Option<&str>,
    by_graph: &TriplesByGraph,
) -> Result<Option<Refusal>> {
    let Some(stored) = shapes else {
        return Ok(None);
    };
    let split = crate::shacl::split_shapes_by_policy(stored);
    for (iri, turtle) in by_graph {
        let g = graph_id(store, iri)?;
        let feedback = crate::shacl_context::validate_with_store_context_in_graph(
            store,
            &split.reject,
            turtle,
            g,
        )?;
        if !feedback.conforms {
            return Ok(Some(Refusal {
                gate: "shacl",
                reason: format!(
                    "SHACL validation failed: {} violation(s): {}. No facts were written",
                    feedback.violations,
                    refusal_detail(&feedback)
                ),
                graph: g,
            }));
        }
        if split.has_emit {
            let observed = crate::shacl_context::validate_with_store_context_in_graph(
                store,
                &split.emit,
                turtle,
                g,
            )?;
            crate::shacl_context::queue_emit_violations(store, &observed);
        }
    }
    Ok(None)
}

#[cfg(not(feature = "shacl"))]
fn check_shapes(
    _store: &Store,
    _shapes: Option<&str>,
    _by_graph: &TriplesByGraph,
) -> Result<Option<Refusal>> {
    Ok(None)
}

#[cfg(feature = "shacl")]
fn check_retractions(
    store: &Store,
    shapes: Option<&str>,
    asserted: &TriplesByGraph,
    removed: &TriplesByGraph,
) -> Result<Option<Refusal>> {
    // A newer refusal than the shapes gate, so it follows
    // `shacl.validate_on_write`: turning the flag off restores the previous
    // /update behaviour exactly.
    if !store.shacl_config().validate_on_write {
        return Ok(None);
    }
    let Some(stored) = shapes else {
        return Ok(None);
    };
    let reject = crate::shacl::split_shapes_by_policy(stored).reject;
    for (iri, removed_nt) in removed {
        let g = graph_id(store, iri)?;
        let feedback = crate::shacl_context::retraction_violations_in_graph(
            store,
            &reject,
            removed_nt,
            asserted.get(iri).map_or("", String::as_str),
            g,
        )?;
        if !feedback.conforms {
            return Ok(Some(Refusal {
                gate: "shacl",
                reason: format!(
                    "SHACL validation failed: {} violation(s) introduced by a deletion: {}. \
                     No facts were written",
                    feedback.violations,
                    refusal_detail(&feedback)
                ),
                graph: g,
            }));
        }
    }
    Ok(None)
}

#[cfg(not(feature = "shacl"))]
fn check_retractions(
    _store: &Store,
    _shapes: Option<&str>,
    _asserted: &TriplesByGraph,
    _removed: &TriplesByGraph,
) -> Result<Option<Refusal>> {
    Ok(None)
}
