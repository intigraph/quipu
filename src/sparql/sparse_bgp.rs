//! Use a proven small literal relation before constructing a whole graph model.

use spargebra::term::{NamedNodePattern, TermPattern, TriplePattern};

use super::pattern_util::{check_eval_budget, triple_pattern_vars};
use super::triple::eval_triple_pattern_limited;
use super::{Bindings, TemporalContext};
use crate::{Result, Store};

pub(super) const SMALL: usize = 128;

pub(super) fn try_eval(
    store: &Store,
    patterns: &[TriplePattern],
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    if patterns.len() < 2
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
        || !ctx.graph.single_graph().is_some_and(|g| g != 0)
    {
        return Ok(None);
    }
    let mut chosen = None;
    for (index, pattern) in patterns.iter().enumerate() {
        // A correlated EXISTS may supply the exact plain identifier through
        // its seed rather than spell it as a literal in the query. Use the
        // same complete ordinary lookup; never infer an absent binding.
        let fixed_literal = matches!(pattern.object, TermPattern::Literal(_))
            || matches!(&pattern.object, TermPattern::Variable(v)
                if matches!(seed.get(v.as_str()), Some(crate::types::Value::Str(_))));
        if !matches!(
            (&pattern.subject, &pattern.predicate),
            (TermPattern::Variable(_), NamedNodePattern::NamedNode(_),)
        ) || !fixed_literal
        {
            continue;
        }
        if patterns.iter().any(|other| {
            other.subject != pattern.subject
                || !matches!(other.predicate, NamedNodePattern::NamedNode(_))
        }) {
            continue; // Only indexed same-subject stars; disconnected joins keep their planner.
        }
        // The ordinary triple reader composes and canonicalizes aliases before
        // counting bound rows, so the completeness probe also works with an
        // unrelated archive mounted. Never probe raw physical ids here.
        let rows = eval_triple_pattern_limited(store, pattern, seed, ctx, Some(SMALL + 1))?;
        if rows.len() <= SMALL {
            chosen = Some((index, rows));
            break;
        }
    }
    let Some((chosen, mut rows)) = chosen else {
        return Ok(None);
    };
    // Headers follow source order regardless of the selected evaluation order.
    let mut variables = Vec::new();
    for pattern in patterns {
        for variable in triple_pattern_vars(pattern) {
            if !variables.contains(&variable) {
                variables.push(variable);
            }
        }
    }
    let remaining: Vec<_> = patterns
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != chosen)
        .collect();
    for (step, (_, pattern)) in remaining.iter().enumerate() {
        let mut next = Vec::new();
        for (index, existing) in rows.iter().enumerate() {
            check_eval_budget(ctx, index, next.len())?;
            let cap = if step + 1 == remaining.len() {
                match ctx.row_limit {
                    Some(cap) if next.len() >= cap => break,
                    Some(cap) => Some(cap - next.len()),
                    None => None,
                }
            } else {
                None
            };
            next.extend(eval_triple_pattern_limited(
                store, pattern, existing, ctx, cap,
            )?);
        }
        rows = next;
    }
    Ok(Some((rows, variables)))
}
