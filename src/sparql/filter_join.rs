//! Drive a nested subject guard from an already-bound selective relation.
//!
//! An independent join otherwise evaluates the guard over unrelated subjects
//! before checking ownership. This is a restricted substitution plan: the
//! driving BGP cannot introduce any variable absent from the guard's BGP.

use std::collections::HashSet;

use spargebra::algebra::{Expression, Function, GraphPattern};
use spargebra::term::NamedNodePattern;

use super::pattern_util::{check_eval_budget, triple_pattern_vars};
use super::{Bindings, TemporalContext};
use crate::{error::Result, store::Store};

// Deliberately exclude volatile/custom functions, SERVICE, subqueries, OPTIONAL,
// paths and other scope-bearing operators. This is not general filter pushdown.
fn stable_expression(expr: &Expression) -> bool {
    match expr {
        Expression::NamedNode(_) | Expression::Literal(_) | Expression::Variable(_) => true,
        Expression::And(a, b)
        | Expression::Or(a, b)
        | Expression::Equal(a, b)
        | Expression::SameTerm(a, b) => stable_expression(a) && stable_expression(b),
        Expression::Not(inner) => stable_expression(inner),
        Expression::In(value, choices) => {
            stable_expression(value) && choices.iter().all(stable_expression)
        }
        Expression::FunctionCall(Function::Str | Function::IsLiteral, args) => {
            args.iter().all(stable_expression)
        }
        Expression::Exists(inner) => stable_pattern(inner),
        _ => false,
    }
}

fn stable_pattern(pattern: &GraphPattern) -> bool {
    match pattern {
        GraphPattern::Bgp { .. } => true,
        GraphPattern::Filter { expr, inner } => stable_expression(expr) && stable_pattern(inner),
        GraphPattern::Join { left, right } => stable_pattern(left) && stable_pattern(right),
        GraphPattern::Graph {
            name: NamedNodePattern::NamedNode(_),
            inner,
        } => stable_pattern(inner),
        _ => false,
    }
}

/// Admit a trailing BGP only when it adds no bindings to the filtered BGP.
/// Every right-hand variable must already be bound in every left BGP solution;
/// otherwise a FILTER/EXISTS could observe a previously unbound variable and
/// change its answer. Preserve the original filter nesting and BGP header order.
fn applicable(left: &GraphPattern, right: &GraphPattern) -> Option<Vec<String>> {
    let GraphPattern::Bgp { patterns: right } = right else {
        return None;
    };
    if right.is_empty() {
        return None;
    }
    let mut filters = Vec::new();
    let mut inner = left;
    while let GraphPattern::Filter { expr, inner: next } = inner {
        if filters.len() == 64 || !stable_expression(expr) {
            return None;
        }
        filters.push(expr);
        inner = next;
    }
    if filters.is_empty() {
        return None;
    }
    let GraphPattern::Bgp { patterns: left } = inner else {
        return None;
    };
    let bound: HashSet<_> = left.iter().flat_map(triple_pattern_vars).collect();
    if right
        .iter()
        .flat_map(triple_pattern_vars)
        .any(|var| !bound.contains(&var))
    {
        return None;
    }
    let mut variables = Vec::new();
    for var in left.iter().flat_map(triple_pattern_vars) {
        if !variables.contains(&var) {
            variables.push(var);
        }
    }
    Some(variables)
}

/// Drive a guard with an existing subject relation, never with new variables.
/// The unmodified guard still evaluates every FILTER against its usual row;
/// only the BGP leaf receives bindings it would have supplied itself.
pub(super) fn try_eval(
    store: &Store,
    left: &GraphPattern,
    right: &GraphPattern,
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    #[cfg(test)]
    if tests::disabled() {
        return Ok(None);
    }
    let Some(vars) = applicable(left, right) else {
        return Ok(None);
    };
    let unbounded = TemporalContext {
        row_limit: None,
        ..ctx.clone()
    };
    let (driving, _) = super::pattern::eval_pattern_seeded(store, right, &unbounded, seed)?;
    if driving.len() > super::bind_join::SUBJECT_BIND_MAX_ROWS {
        return Ok(None);
    }
    let dense = driving.len() > super::sparse_bgp::SMALL
        && seed.is_empty()
        && ctx.valid_at.is_none()
        && ctx.as_of_tx.is_none()
        && !ctx.entails_rdfs
        && ctx.row_limit.is_none()
        && ctx.string_narrows.is_none()
        && !ctx.row_cap.is_some_and(|cap| cap < 50_001)
        && matches!(&ctx.graph, super::GraphScope::Named(ids) if !ids.is_empty() && ids.iter().all(|id| *id > 0));
    #[cfg(test)]
    let dense = dense && !tests::v1_disabled();
    if dense && let Some(rows) = dense_rows(store, left, right, &unbounded, seed)? {
        return Ok(Some((rows, vars)));
    }
    let mut rows = Vec::new();
    for (i, bound) in driving.iter().enumerate() {
        check_eval_budget(ctx, i, rows.len())?;
        let (mut selected, _) =
            super::pattern::eval_pattern_seeded(store, left, &unbounded, bound)?;
        rows.append(&mut selected);
        check_eval_budget(ctx, i, rows.len())?;
    }
    Ok(Some((rows, vars)))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) fn reference_disabled() -> bool {
    tests::disabled() || tests::v1_disabled()
}

fn dense_rows(
    store: &Store,
    left: &GraphPattern,
    right: &GraphPattern,
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<Option<Vec<Bindings>>> {
    let mut filters = Vec::new();
    let mut leaf = left;
    while let GraphPattern::Filter { expr, inner } = leaf {
        filters.push(expr);
        leaf = inner;
    }
    let GraphPattern::Bgp { patterns } = leaf else {
        return Ok(None);
    };
    let GraphPattern::Bgp { patterns: trailing } = right else {
        return Ok(None);
    };
    if patterns.len() + trailing.len() > 8 {
        return Ok(None);
    }
    let mut patterns = patterns.clone();
    patterns.extend(trailing.iter().cloned());
    let Some((mut rows, _)) = super::batch_star::try_eval(store, &patterns, ctx, seed)? else {
        return Ok(None);
    };
    for expr in filters.into_iter().rev() {
        rows = match super::batch_absence::filter(store, expr, &rows, ctx)? {
            Some(rows) => rows,
            None => {
                let mut selected = Vec::new();
                for (i, row) in rows.into_iter().enumerate() {
                    check_eval_budget(ctx, i, selected.len())?;
                    if super::filter::eval_filter(store, expr, &row, ctx)? {
                        selected.push(row);
                    }
                }
                selected
            }
        };
    }
    Ok(Some(rows))
}
