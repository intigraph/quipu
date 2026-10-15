//! Store-aware ORDER BY keys, evaluated once per solution.
use super::Bindings;
use crate::{error::Result, store::Store, types::Value};
use spargebra::algebra::OrderExpression;

pub(super) fn sort_rows(
    store: &Store,
    rows: Vec<Bindings>,
    expression: &[OrderExpression],
) -> Result<Vec<Bindings>> {
    // Evaluate each key once per solution, including volatile expressions.
    // Ref IDs encode insertion order; only resolved strings order IRIs.
    let mut iri_keys: std::collections::HashMap<i64, std::sync::Arc<str>> =
        std::collections::HashMap::new();
    let mut keyed = rows
        .into_iter()
        .map(|row| {
            let keys = expression
                .iter()
                .map(|order| {
                    let (OrderExpression::Asc(expr) | OrderExpression::Desc(expr)) = order;
                    let value = super::filter::eval_expr(store, expr, &row);
                    let iri = if let Some(Value::Ref(id)) = &value {
                        let lexical = match iri_keys.entry(*id) {
                            std::collections::hash_map::Entry::Occupied(entry) => {
                                entry.get().clone()
                            }
                            std::collections::hash_map::Entry::Vacant(entry) => {
                                let lexical: std::sync::Arc<str> = store.resolve(*id)?.into();
                                entry.insert(lexical.clone());
                                lexical
                            }
                        };
                        Some(lexical)
                    } else {
                        None
                    };
                    Ok((value, iri))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((row, keys))
        })
        .collect::<Result<Vec<_>>>()?;
    keyed.sort_by(|(_, a), (_, b)| {
        for ((order, (va, ia)), (vb, ib)) in expression.iter().zip(a).zip(b) {
            let cmp = match (ia, ib) {
                (Some(a), Some(b)) => a.cmp(b),
                _ => super::aggregate::compare_option_values(va, vb),
            };
            let cmp = if matches!(order, OrderExpression::Asc(_)) {
                cmp
            } else {
                cmp.reverse()
            };
            if cmp != std::cmp::Ordering::Equal {
                return cmp;
            }
        }
        std::cmp::Ordering::Equal
    });
    Ok(keyed.into_iter().map(|(row, _)| row).collect())
}
