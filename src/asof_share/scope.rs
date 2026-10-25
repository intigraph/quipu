//! Which graphs a cut or a replication stream may serve, and the request's
//! scope lists. Shared by both, so they refuse exactly the same graphs.

use super::support::{frozen_pack, sorted_unique};
use super::{CutError, ROOT, invalid};
use crate::Store;

/// The scope and context lists of a request, each sorted and duplicate-free:
/// 1..=64 graphs in all, none in both. Shared with the replication stream.
///
/// # Errors
/// [`CutError::Refused`] for a list that breaks those rules.
pub fn scope_lists(
    graphs: &[String],
    context_graphs: &[String],
) -> std::result::Result<(Vec<String>, Vec<String>), CutError> {
    let scope = sorted_unique(graphs, "graphs")?;
    let context = sorted_unique(context_graphs, "context_graphs")?;
    if scope.is_empty() || scope.len() + context.len() > 64 {
        return Err(invalid(
            "as-of share requires 1..=64 explicit named graphs, context included",
        ));
    }
    if scope.iter().any(|iri| context.contains(iri)) {
        return Err(invalid("a graph is both in scope and in context"));
    }
    Ok((scope, context))
}

/// Each scope and context graph with its id, or why one cannot be served:
/// registered, committed, not a dataset, not the source-local graph-metadata
/// graph, not contributed by an attached pack, and within the label floor.
/// ROOT is served only when named. Shared with the replication stream, so both
/// refuse exactly the same graphs. A FROZEN graph passes here: the cut reads it
/// from its pack, and the stream refuses it with [`refuse_frozen`].
///
/// # Errors
/// [`CutError::Refused`] naming the graph that cannot be served.
pub fn resolve_graphs(
    store: &Store,
    scope: &[String],
    context: &[String],
) -> std::result::Result<Vec<(String, i64)>, CutError> {
    let mut selected = Vec::new();
    for iri in scope.iter().chain(context) {
        // The graph-metadata graph is the source's own record (labels,
        // lifecycle). It is source-local, and a freeze writes it in the very
        // transaction a frozen graph's records carry, so a cut naming it
        // would put two graphs' rows in one source transaction.
        if iri == crate::namespace::META_GRAPH_IRI {
            return Err(invalid(format!(
                "the graph-metadata graph is source-local and is never cut: {iri}"
            )));
        }
        // ROOT is never implied; a caller names it explicitly to cut it.
        if iri == ROOT {
            store.check_label_floor(&[0])?;
            selected.push((iri.clone(), 0));
            continue;
        }
        oxrdf::NamedNode::new(iri.clone()).map_err(|e| invalid(format!("graph IRI: {e}")))?;
        let Some(graph) = store.registered_graph_id(iri)? else {
            return Err(invalid(format!("unregistered graph: {iri}")));
        };
        if store.is_dataset(iri)? {
            return Err(invalid(format!("dataset IRI, not a graph: {iri}")));
        }
        if store.graph_class(graph)?.as_deref() != Some("committed") {
            return Err(invalid(format!(
                "as-of share requires a committed graph, not an overlay or fork: {iri}"
            )));
        }
        // A graph CONTRIBUTED by an attachment belongs to that attachment's
        // owner and is refused.
        if store.attached_graphs().contains(&graph) {
            return Err(invalid(format!(
                "graph is contributed by an attached pack, not cut by this store: {iri}"
            )));
        }
        store.check_label_floor(&[graph])?;
        selected.push((iri.clone(), graph));
    }
    Ok(selected)
}

/// The replication stream serves graphs from main only. A frozen graph's rows
/// live in its pack, which the stream cannot serve yet, so it is refused there
/// while the cut reads it from the pack.
///
/// # Errors
/// [`CutError::Refused`] naming the frozen graph.
pub fn refuse_frozen(store: &Store, graphs: &[(String, i64)]) -> std::result::Result<(), CutError> {
    for (iri, graph) in graphs {
        if *graph != 0 && frozen_pack(store, iri)?.is_some() {
            return Err(invalid(format!(
                "graph is frozen into a pack, which the replication stream cannot serve yet: {iri}"
            )));
        }
    }
    Ok(())
}
