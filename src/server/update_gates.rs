//! The write gates `/knot` enforces, applied to what a SPARQL `/update` asserts
//! (aegis-1hfyk5).
//!
//! `/update` evaluated the request with Oxigraph and transacted the diff with no
//! gate at all. So on a path seeds uses for the shared board, the closed
//! vocabulary and every loaded shape were skipped. Measured on 674f3700 (ian,
//! aegis-bqgdr3 C2): an unknown `rdf:type` and a seed missing a required
//! property were refused by `/knot` and STORED by `/update`.
//!
//! The gates run on the ASSERTED quads, before any term is interned or any
//! transaction begins, so a refusal stores nothing. Same order and semantics as
//! `/knot`: the vocabulary first, then each destination graph's asserted triples
//! against the stored shapes with that graph's store context. The reject half
//! blocks; the emit half only queues `shacl.violation` events. A retraction is
//! judged only where it can break a node: a subject losing a value on a
//! required path must not end up with a violation its pre-state did not have
//! (aegis-g51c C12). Without that, write-time debt admission would accept a
//! node made non-conforming by a delete as if it were old debt.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use oxigraph::model::{GraphName, Quad};

use super::super::base::AppError;

/// Refuse the update if what it asserts fails the vocabulary or the loaded
/// shapes. `Ok(())` means it may be transacted.
pub(super) fn enforce<'a>(
    store: &quipu::Store,
    asserted: impl Iterator<Item = &'a Quad>,
    removed: impl Iterator<Item = &'a Quad>,
    actor: Option<&str>,
    source: &str,
) -> Result<(), AppError> {
    // Per destination graph: N-Triples of what lands there. N-Triples is valid
    // Turtle, which is what both gates take.
    let by_graph = n_triples_by_graph(asserted);
    let removed_by_graph = n_triples_by_graph(removed);
    if !by_graph.is_empty() {
        let all: String = by_graph.values().map(String::as_str).collect();
        quipu::vocabulary::enforce_turtle(store, &all)?;
        shapes(store, &by_graph, actor, source)?;
    }
    if removed_by_graph.is_empty() {
        return Ok(());
    }
    retractions(store, &by_graph, &removed_by_graph, actor, source)
}

fn n_triples_by_graph<'a>(quads: impl Iterator<Item = &'a Quad>) -> BTreeMap<String, String> {
    let mut by_graph: BTreeMap<String, String> = BTreeMap::new();
    for quad in quads {
        let key = match &quad.graph_name {
            GraphName::NamedNode(n) => n.as_str().to_owned(),
            _ => String::new(),
        };
        let _ = writeln!(
            by_graph.entry(key).or_default(),
            "{} {} {} .",
            quad.subject,
            quad.predicate,
            quad.object
        );
    }
    by_graph
}

/// The graph id a destination IRI resolves to; a graph this update creates
/// has no facts of its own yet, so ROOT is its whole context.
fn graph_id(store: &quipu::Store, iri: &str) -> Result<i64, AppError> {
    Ok(if iri.is_empty() {
        quipu::schema::ROOT_GRAPH
    } else {
        store.lookup(iri)?.unwrap_or(quipu::schema::ROOT_GRAPH)
    })
}

fn refusal_detail(feedback: &quipu::shacl::ValidationFeedback) -> String {
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
fn retractions(
    store: &quipu::Store,
    asserted: &BTreeMap<String, String>,
    removed: &BTreeMap<String, String>,
    actor: Option<&str>,
    source: &str,
) -> Result<(), AppError> {
    // A new refusal, so it follows `shacl.validate_on_write`: turning the flag
    // off restores the previous /update behaviour exactly.
    if !store.shacl_config().validate_on_write {
        return Ok(());
    }
    let Some(stored) = store.get_combined_shapes()? else {
        return Ok(());
    };
    let reject = quipu::shacl::split_shapes_by_policy(&stored).reject;
    for (iri, removed_nt) in removed {
        let g = graph_id(store, iri)?;
        let feedback = quipu::shacl_context::retraction_violations_in_graph(
            store,
            &reject,
            removed_nt,
            asserted.get(iri).map_or("", String::as_str),
            g,
        )?;
        if !feedback.conforms {
            let reason = format!(
                "SHACL validation failed: {} violation(s) introduced by a deletion: {}. \
                 No facts were written",
                feedback.violations,
                refusal_detail(&feedback)
            );
            let now = quipu::time::now_iso();
            store.record_gate_refusal("shacl", &reason, g, actor, Some(source), &now);
            return Err(quipu::Error::InvalidValue(reason).into());
        }
    }
    Ok(())
}

#[cfg(not(feature = "shacl"))]
fn retractions(
    _store: &quipu::Store,
    _asserted: &BTreeMap<String, String>,
    _removed: &BTreeMap<String, String>,
    _actor: Option<&str>,
    _source: &str,
) -> Result<(), AppError> {
    Ok(())
}

#[cfg(feature = "shacl")]
fn shapes(
    store: &quipu::Store,
    by_graph: &BTreeMap<String, String>,
    actor: Option<&str>,
    source: &str,
) -> Result<(), AppError> {
    let Some(stored) = store.get_combined_shapes()? else {
        return Ok(());
    };
    let split = quipu::shacl::split_shapes_by_policy(&stored);
    let now = quipu::time::now_iso();
    for (iri, turtle) in by_graph {
        // A graph this update creates has no facts of its own yet; ROOT is its
        // whole context, as for a /knot into a fresh graph.
        let g = graph_id(store, iri)?;
        let feedback = quipu::shacl_context::validate_with_store_context_in_graph(
            store,
            &split.reject,
            turtle,
            g,
        )?;
        if !feedback.conforms {
            let detail = refusal_detail(&feedback);
            let reason = format!(
                "SHACL validation failed: {} violation(s): {detail}. No facts were written",
                feedback.violations
            );
            store.record_gate_refusal("shacl", &reason, g, actor, Some(source), &now);
            return Err(quipu::Error::InvalidValue(reason).into());
        }
        if split.has_emit {
            let observed = quipu::shacl_context::validate_with_store_context_in_graph(
                store,
                &split.emit,
                turtle,
                g,
            )?;
            quipu::shacl_context::queue_emit_violations(store, &observed);
        }
    }
    Ok(())
}

#[cfg(not(feature = "shacl"))]
fn shapes(
    _store: &quipu::Store,
    _by_graph: &BTreeMap<String, String>,
    _actor: Option<&str>,
    _source: &str,
) -> Result<(), AppError> {
    Ok(())
}
