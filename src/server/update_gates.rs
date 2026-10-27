//! The write gates `/knot` enforces, applied to what a SPARQL `/update` asserts
//! (aegis-1hfyk5).
//!
//! `/update` evaluated the request with Oxigraph and transacted the diff with no
//! gate at all. So on a path seeds uses for the shared board, the closed
//! vocabulary and every loaded shape were skipped. Measured on 674f3700 (ian,
//! aegis-bqgdr3 C2): an unknown `rdf:type` and a seed missing a required
//! property were refused by `/knot` and STORED by `/update`.
//!
//! The gate itself lives in [`quipu::write_gate`], shared with proposal
//! migrations (aegis-esd); this adapter turns the Oxigraph diff into the
//! per-graph N-Triples the gate judges and records a refusal.

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
    if by_graph.is_empty() && removed_by_graph.is_empty() {
        return Ok(());
    }
    let shapes = store.get_combined_shapes()?;
    match quipu::write_gate::check(store, shapes.as_deref(), &by_graph, &removed_by_graph)? {
        None => Ok(()),
        Some(refusal) => Err(refusal.record(store, actor, source).into()),
    }
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
