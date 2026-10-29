//! The commit half of a write: gates, datums, SHACL, transaction, registry.
//!
//! Shared by `/update` (after Oxigraph evaluates the update) and `/patch`
//! (whose change-set is already a ground delete/insert pair), so a change-set
//! faces exactly the gates every SPARQL update faces (aegis-4mk).

use std::collections::HashMap;
use std::time::Instant;

use oxigraph::model::{GraphName, Quad};

use super::{Attribution, update_gates, update_observe::UpdateTimes};
use crate::base::AppError;

/// Per-graph datums, ROOT first, and the `(graph, tx)` each batch committed as.
pub(crate) type Committed = (Vec<(i64, Vec<quipu::store::Datum>)>, Vec<(i64, i64)>);

/// Gate, then transact, `removed` and `asserted`. `valid_from` is the
/// asserted statements' valid time (default: the commit instant); a refusal
/// by any gate writes nothing.
pub(super) fn commit<'a>(
    store: &mut quipu::Store,
    removed: impl Iterator<Item = &'a Quad> + Clone,
    asserted: impl Iterator<Item = &'a Quad> + Clone,
    attribution: &Attribution,
    valid_from: Option<&str>,
    times: &mut UpdateTimes,
) -> Result<Committed, AppError> {
    let mut phase = Instant::now();
    let mut lap = |slot: &mut f64| {
        *slot = phase.elapsed().as_secs_f64();
        phase = Instant::now();
    };
    // The write gates /knot enforces, on what this update asserts, before any
    // term is interned or any transaction begins (aegis-1hfyk5).
    update_gates::enforce(
        store,
        asserted.clone(),
        removed.clone(),
        attribution.actor.as_deref(),
        &attribution.source,
    )?;
    lap(&mut times.gates);
    let now = quipu::time::now_iso();
    let from = valid_from.unwrap_or(&now).to_string();
    let mut graph_ids = HashMap::from([(GraphName::DefaultGraph, 0)]);
    let mut changes: HashMap<i64, Vec<quipu::store::Datum>> = HashMap::new();
    for (quads, op) in [
        (removed.collect::<Vec<_>>(), quipu::Op::Retract),
        (asserted.collect::<Vec<_>>(), quipu::Op::Assert),
    ] {
        for quad in quads {
            let graph_id = graph_id(store, &mut graph_ids, &quad.graph_name)?;
            let (valid_from, valid_to) = if op == quipu::Op::Assert {
                (from.clone(), None)
            } else {
                (now.clone(), Some(now.clone()))
            };
            changes
                .entry(graph_id)
                .or_default()
                .push(quipu::store::Datum {
                    entity: quipu::rdf::intern_subject(store, &quad.subject)?,
                    attribute: store.intern(quad.predicate.as_str())?,
                    value: quipu::rdf::term_to_value(store, &quad.object)?,
                    valid_from,
                    valid_to,
                    op,
                });
        }
    }
    let mut batches: Vec<_> = changes.into_iter().collect();
    // Deterministic commit (and report) order across graphs; ROOT first.
    batches.sort_unstable_by_key(|(graph, _)| *graph);
    lap(&mut times.prep);
    let shapes = store
        .get_combined_shapes()?
        .map(|s| quipu::shacl::split_shapes_by_policy(&s).reject);
    lap(&mut times.shapes);
    let txs = store.with_write_shapes(shapes.as_deref(), |store| {
        store.transact_graph_batches_tx(
            &batches,
            &now,
            attribution.actor.as_deref(),
            Some(&attribution.source),
        )
    })?;
    lap(&mut times.commit);
    // Register every named graph this write asserted into, so the next
    // update's dataset includes it (aegis-e9o5ci). After the commit, so a
    // refused write leaves no empty registry row behind.
    for (graph_id, datums) in &batches {
        if datums.iter().any(|d| d.op == quipu::Op::Assert) {
            store.graph_ensure_registered(*graph_id)?;
        }
    }
    Ok((batches, txs))
}

/// [`commit`] for a caller outside `/update` (`/patch`), which keeps no phase timings.
pub(crate) fn commit_untimed<'a>(
    store: &mut quipu::Store,
    removed: impl Iterator<Item = &'a Quad> + Clone,
    asserted: impl Iterator<Item = &'a Quad> + Clone,
    attribution: &Attribution,
    valid_from: Option<&str>,
) -> Result<Committed, AppError> {
    commit(
        store,
        removed,
        asserted,
        attribution,
        valid_from,
        &mut UpdateTimes::default(),
    )
}

fn graph_id(
    store: &quipu::Store,
    ids: &mut HashMap<GraphName, i64>,
    graph: &GraphName,
) -> Result<i64, AppError> {
    if let Some(id) = ids.get(graph) {
        return Ok(*id);
    }
    let GraphName::NamedNode(name) = graph else {
        return Err(
            quipu::Error::InvalidValue("blank-node graph names are unsupported".into()).into(),
        );
    };
    let id = store.intern(name.as_str())?;
    ids.insert(graph.clone(), id);
    Ok(id)
}
