//! Property-path edges over a composed store (aegis-n0wc.5).
//!
//! One IRI may carry an id per attached file (an alias), so a single edge must
//! match every id of its predicate and fixed endpoints, and return canonical
//! ids. Split from `property_path.rs` to keep that file under the size ratchet.

use crate::error::Result;
use crate::store::Store;
use crate::types::Value;

use super::super::TemporalContext;
use super::{add_graph_condition, add_temporal_conditions, push_unique, query_pairs};

/// [`eval_single_edge`] over a composed store. One IRI may carry an id per
/// file (an alias), so the predicate and any fixed endpoint match EVERY id of
/// their IRI, and returned pairs are canonical. Otherwise a path through a
/// shared IRI stops at the file boundary
/// (`property_paths_follow_aliases_across_term_spaces`).
pub(super) fn eval_single_edge_composed(
    store: &Store,
    pred_iri: &str,
    fixed_subj: Option<i64>,
    fixed_obj: Option<i64>,
    ctx: &TemporalContext,
) -> Result<Vec<(i64, i64)>> {
    let preds = store.lookup_all(pred_iri)?;
    if preds.is_empty() {
        return Ok(vec![]);
    }
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let placeholders = |params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
                        values: Vec<Box<dyn rusqlite::types::ToSql>>| {
        let start = params.len();
        let marks: Vec<String> = (0..values.len())
            .map(|i| format!("?{}", start + i + 1))
            .collect();
        params.extend(values);
        marks.join(",")
    };
    let a = placeholders(
        &mut params,
        preds
            .iter()
            .map(|id| Box::new(*id) as Box<dyn rusqlite::types::ToSql>)
            .collect(),
    );
    let mut conds = vec![format!("a IN ({a})"), "op = 1".to_string()];
    add_graph_condition(&mut conds, &mut params, ctx)?;
    if let Some(s) = fixed_subj {
        let ids = alias_ids(store, s)?;
        let e = placeholders(
            &mut params,
            ids.into_iter()
                .map(|id| Box::new(id) as Box<dyn rusqlite::types::ToSql>)
                .collect(),
        );
        conds.push(format!("e IN ({e})"));
    }
    if let Some(o) = fixed_obj {
        let ids = alias_ids(store, o)?;
        let v = placeholders(
            &mut params,
            ids.into_iter()
                .map(|id| Box::new(Value::Ref(id).to_bytes()) as Box<dyn rusqlite::types::ToSql>)
                .collect(),
        );
        conds.push(format!("v IN ({v})"));
    }
    add_temporal_conditions(&mut conds, &mut params, ctx);
    let mut out = Vec::new();
    for (s, o) in query_pairs(store, &conds, &params)? {
        push_unique(&mut out, (store.canonical_id(s)?, store.canonical_id(o)?));
    }
    Ok(out)
}

/// Every id the composed dictionaries hold for `id`'s IRI (itself included).
fn alias_ids(store: &Store, id: i64) -> Result<Vec<i64>> {
    let iri = store.resolve(id)?;
    if iri.starts_with("_:") {
        return Ok(vec![id]);
    }
    let mut ids = store.lookup_all(&iri)?;
    if !ids.contains(&id) {
        ids.push(id);
    }
    Ok(ids)
}
