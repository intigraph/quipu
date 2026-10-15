//! Resident read-model triple matching; kept separate from the SQL leaf.
use crate::sparql::Bindings;
use crate::sparql::pattern_util::{
    resolve_object_pattern, resolve_predicate_pattern, resolve_subject_pattern,
};
use crate::sparql::triple_bind::{MatchedRow, bind_row};
use crate::{Result, Store, Value};
use spargebra::term::TriplePattern;

/// Evaluate one triple pattern against the resident read model instead of SQL.
///
/// Only ever reached when `read_model_applicable` admits (see
/// `src/store/read_model.rs`), which is what makes the shortcuts here sound:
/// the graph is plain ROOT so no graph variable binds and `g_id` is always
/// `None`, and there are no attachments so `canonical_id` is the identity and
/// `lookup_all` degenerates to `lookup`.
///
/// Rows go through the same [`bind_row`] as the SQL path, so the two cannot
/// disagree about what a triple binds to.
///
/// Candidates are sorted by `(e, a, v)`. The SQL this replaces carries no
/// `ORDER BY`, so its order was incidental — index order, usually `idx_eavt`.
/// Sorting makes the fast path deterministic rather than merely different.
pub(in crate::sparql) fn eval_triple_pattern_from_model(
    store: &Store,
    tp: &TriplePattern,
    bindings: &Bindings,
    graph: i64,
) -> Result<Vec<Bindings>> {
    let subject = match resolve_subject_pattern(store, &tp.subject, bindings)? {
        Some(iri) => match store.lookup(&iri)? {
            Some(id) => Some(id),
            None => return Ok(vec![]), // not in the dictionary -> no matches
        },
        None => None,
    };
    let predicate = match resolve_predicate_pattern(store, &tp.predicate, bindings)? {
        Some(iri) => match store.lookup(&iri)? {
            Some(id) => Some(id),
            None => return Ok(vec![]),
        },
        None => None,
    };
    let object = resolve_object_pattern(store, &tp.object, bindings)?;

    let model = store.read_model_for(graph)?;
    let mut candidates: Vec<(i64, i64, Value)> = match (subject, predicate, &object) {
        (Some(e), Some(a), Some(v)) => {
            if model.contains(store, e, a, v)? {
                vec![(e, a, v.clone())]
            } else {
                vec![]
            }
        }
        (Some(e), Some(a), None) => model
            .by_subject(store, e)?
            .iter()
            .filter(|(pa, _)| *pa == a)
            .map(|(pa, v)| (e, *pa, v.clone()))
            .collect(),
        (Some(e), None, Some(v)) => model
            .by_subject(store, e)?
            .iter()
            .filter(|(_, pv)| *pv == *v)
            .map(|(pa, pv)| (e, *pa, pv.clone()))
            .collect(),
        (Some(e), None, None) => model
            .by_subject(store, e)?
            .iter()
            .map(|(pa, pv)| (e, *pa, pv.clone()))
            .collect(),
        (None, Some(a), Some(v)) => model
            .by_predicate_object(store, a, v)?
            .iter()
            .map(|e| (*e, a, v.clone()))
            .collect(),
        (None, Some(a), None) => model
            .by_predicate(store, a)?
            .iter()
            .map(|(e, pv)| (*e, a, pv.clone()))
            .collect(),
        (None, None, Some(v)) => model
            .by_object(store, v)?
            .iter()
            .map(|(e, a)| (*e, *a, v.clone()))
            .collect(),
        (None, None, None) => model.triples(store)?,
    };
    candidates.sort_unstable_by_key(|l| (l.0, l.1, l.2.term_key()));

    let mut results = Vec::with_capacity(candidates.len());
    for (e_id, a_id, v) in candidates {
        let matched = MatchedRow {
            e_id,
            a_id,
            v,
            g_id: None,
        };
        if let Some(row) = bind_row(store, tp, bindings, matched, None)? {
            results.push(row);
        }
    }
    Ok(results)
}
