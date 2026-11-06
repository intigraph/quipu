//! Evaluation entry points, safe response caps, and profiling boundary.

use super::{Bindings, GraphPattern, Result, Store, TemporalContext};
use crate::sparql::profile;

/// Whether a `Slice` cap may be pushed through this subtree to the BGP leaf
/// (quipu-0lr).
///
/// True only for chains where every intermediate operator is row-monotonic
/// AND prefix-safe — it passes each input row through at most once, in order,
/// dropping none: `Project` (1:1 column projection) and `Reduced` (advisory).
/// Everything else is a hard no: `Filter` drops rows (the leaf would need to
/// overshoot by an unknowable factor), `Distinct` collapses them, `OrderBy`
/// needs the full set before any row is final, joins multiply, and `Union`
/// concatenates two branches that would each need their own budget. When this
/// says no, evaluation is exactly what it was before the pushdown existed.
pub(crate) fn limit_pushdown_safe(pattern: &GraphPattern) -> bool {
    match pattern {
        GraphPattern::Bgp { .. } => true,
        GraphPattern::Project { inner, .. } | GraphPattern::Reduced { inner } => {
            limit_pushdown_safe(inner)
        }
        _ => false,
    }
}

pub(super) fn ordered_limit_safe(pattern: &GraphPattern) -> bool {
    match pattern {
        GraphPattern::OrderBy { .. } => true,
        GraphPattern::Project { inner, .. } => ordered_limit_safe(inner),
        _ => false,
    }
}

/// Evaluate a graph pattern, returning rows and the variable names encountered.
pub fn eval_pattern(
    store: &Store,
    pattern: &GraphPattern,
    ctx: &TemporalContext,
) -> Result<(Vec<Bindings>, Vec<String>)> {
    eval_pattern_seeded(store, pattern, ctx, &Bindings::default())
}

/// Evaluate a top-level result with the service response ceiling pushed to the
/// SQL leaf only when doing so preserves SPARQL semantics.
///
/// `query_result` historically truncated only after the evaluator had
/// materialized every solution. On the production graph, a plain `?s ?p ?o`
/// scan therefore built hundreds of thousands of bindings to return 10,000,
/// consuming seconds and gigabytes under concurrency. A prefix-safe plan can
/// stop at `max_sparql_rows + 1` (the extra row proves truncation) without
/// changing which prefix is returned. Any operator that can drop, reorder,
/// combine, or collapse rows keeps the uncapped evaluator.
pub fn eval_pattern_result_limited(
    store: &Store,
    pattern: &GraphPattern,
    ctx: &TemporalContext,
) -> Result<(Vec<Bindings>, Vec<String>)> {
    let Some(cap) = ctx.result_limit else {
        return eval_pattern(store, pattern, ctx);
    };
    if !limit_pushdown_safe(pattern) {
        return eval_pattern(store, pattern, ctx);
    }
    let capped = TemporalContext {
        row_limit: Some(ctx.row_limit.map_or(cap, |existing| existing.min(cap))),
        ..ctx.clone()
    };
    eval_pattern(store, pattern, &capped)
}

/// Evaluate a graph pattern SEEDED with pre-bound variables (`seed`), which
/// initialise every leaf (BGP / property path) so evaluation is CONSTRAINED by
/// them — the SPARQL EXISTS substitution semantics. A bound ?s makes a path
/// traverse only from that ?s, not the whole graph (the unseeded
/// per-outer-row re-eval held the store mutex O(n x `inner_eval`)). An empty seed
/// reproduces the original unconstrained evaluation exactly. The deadline check
/// stays here so seeded recursion is budget-bounded like the wrapper was.
pub fn eval_pattern_seeded(
    store: &Store,
    pattern: &GraphPattern,
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<(Vec<Bindings>, Vec<String>)> {
    let timer = profile::begin();
    let result = super::eval_pattern_seeded_inner(store, pattern, ctx, seed);
    profile::finish(
        pattern,
        timer,
        result.as_ref().ok().map(|(rows, _)| rows.len()),
    );
    result
}
