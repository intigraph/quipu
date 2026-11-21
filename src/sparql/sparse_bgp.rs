//! Use a proven small fixed-object relation before constructing a whole graph model.
//!
//! First pass (unchanged): the first fixed literal relation of at most
//! [`SMALL`] rows drives. Second pass (aegis-prna R3): otherwise the SMALLEST
//! fixed literal or non-type IRI relation of at most [`WIDE`] rows drives.
//! Without it, a seeds owner star such as `?s status "open" . ?s agent <p>`
//! (1,229 and 390 rows) fell through to the resident whole-graph read model:
//! a full current-fact scan per reader handle plus write catch-up, measured
//! at 0.8-10 s per query and the dominant owner-rows store hold in production.
//!
//! aegis-n0wc C: a follower may also be a whole-subject read `?s ?p ?o` once the
//! driver has bound `?s` (a bound-subject index probe per driver row). The
//! seeds family read `?s <parentItem> <item> . ?s ?p ?o` (314 comment facts on
//! a busy item) otherwise fell through to the whole-graph model: 3.5-4.2 s of
//! store per read, three reads per crew comment. The driver itself must still
//! be a named, fixed relation, and RDFS entailment keeps the ordinary planner.

use spargebra::term::{NamedNodePattern, TermPattern, TriplePattern};

use super::pattern_util::{check_eval_budget, triple_pattern_vars};
use super::triple::eval_triple_pattern_limited;
use super::{Bindings, TemporalContext};
use crate::{Result, Store};

pub(super) const SMALL: usize = 128;
/// Largest driver of the second pass: each remaining pattern then costs at
/// most this many bound-subject index probes, never a whole-graph build.
pub(super) const WIDE: usize = 1024;

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
        if !indexed_star(patterns, ctx) {
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
    if chosen.is_none() && !patterns.iter().any(|p| p.subject != patterns[0].subject) {
        chosen = smallest_fixed_driver(store, patterns, ctx, seed)?;
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

/// The smallest fixed-object relation of at most [`WIDE`] rows, if any.
///
/// Only same-subject stars over named predicates reach here (checked by the
/// caller and below). `rdf:type` objects keep the entailment-aware planner,
/// and an IRI object is skipped under RDFS entailment, where a sub-property
/// could contribute rows that a direct lookup cannot see.
fn smallest_fixed_driver(
    store: &Store,
    patterns: &[TriplePattern],
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<Option<(usize, Vec<Bindings>)>> {
    if !indexed_star(patterns, ctx) {
        return Ok(None);
    }
    let mut best: Option<(usize, Vec<Bindings>)> = None;
    for (index, pattern) in patterns.iter().enumerate() {
        let NamedNodePattern::NamedNode(predicate) = &pattern.predicate else {
            continue;
        };
        // An if-chain, not a match: TermPattern's variant set depends on
        // features, so a wildcard arm trips clippy on one build or the other.
        let fixed = if let TermPattern::Variable(v) = &pattern.object {
            matches!(seed.get(v.as_str()), Some(crate::types::Value::Str(_)))
        } else if matches!(pattern.object, TermPattern::NamedNode(_)) {
            !ctx.entails_rdfs && predicate.as_str() != crate::namespace::RDF_TYPE
        } else {
            matches!(pattern.object, TermPattern::Literal(_))
        };
        if !fixed {
            continue;
        }
        let bound = best.as_ref().map_or(WIDE, |(_, rows)| rows.len());
        let rows = eval_triple_pattern_limited(store, pattern, seed, ctx, Some(bound + 1))?;
        if rows.len() <= bound && best.as_ref().is_none_or(|(_, b)| rows.len() < b.len()) {
            best = Some((index, rows));
        }
    }
    Ok(best)
}

/// A same-subject star every pattern of which is an indexed probe once the
/// subject is bound: a named predicate, or (aegis-n0wc C) a whole-subject
/// `?s ?p ?o` follower whose predicate variable is not the subject. At least
/// one pattern must be named, because only a named relation can drive. Under
/// RDFS entailment a variable predicate could need inferred facts that a
/// direct lookup does not see, so it keeps the ordinary planner.
fn indexed_star(patterns: &[TriplePattern], ctx: &TemporalContext) -> bool {
    let TermPattern::Variable(subject) = &patterns[0].subject else {
        return false;
    };
    let mut named = false;
    for pattern in patterns {
        if pattern.subject != patterns[0].subject {
            return false;
        }
        match &pattern.predicate {
            NamedNodePattern::NamedNode(_) => named = true,
            NamedNodePattern::Variable(p) if !ctx.entails_rdfs && p != subject => {}
            NamedNodePattern::Variable(_) => return false,
        }
    }
    named
}
