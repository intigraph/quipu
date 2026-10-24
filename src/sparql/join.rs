//! Hash-join BGP evaluation over the in-memory read model (quipu-0lr).
//!
//! Split from `triple.rs` (quipu-sd1): the planner and executor are a
//! self-contained layer over the per-pattern row sets `triple.rs` produces.

use std::collections::{HashMap, HashSet};

use spargebra::term::TriplePattern;

use crate::error::Result;
use crate::store::Store;
use crate::types::Value;

use super::pattern_util::{check_eval_budget, triple_pattern_vars};
use super::triple::{estimate_triple_pattern_from_model, eval_triple_pattern_from_model};
use super::{Bindings, TemporalContext};

/// Evaluate a BGP by hash-joining each pattern's rows, rather than re-evaluating
/// the pattern once per accumulated row.
///
/// The nested loop in [`eval_bgp`] is quadratic by construction: pattern *k* is
/// evaluated once for every row pattern *k-1* produced. Making each evaluation
/// cheap — which the read model does — shrinks the constant and leaves the
/// shape. This changes the shape.
///
/// Each pattern is evaluated ONCE against the seed, then joined to the
/// accumulated rows on whatever variables they share — unless the rows so far
/// are fewer than the pattern's facts, when it is probed once per row instead. Only used when the read
/// model is applicable, because there an unbound evaluation is a hash lookup;
/// against SQL it would trade a selective indexed query for a table scan.
pub(super) fn eval_bgp_hash_join(
    store: &Store,
    patterns: &[TriplePattern],
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<(Vec<Bindings>, Vec<String>)> {
    // ORDERING the joins by cardinality is nearly free (quipu-0lr): the
    // counts are index lengths, not statistics. Source order stops mattering
    // — a pathological ordering folds the same joins as a good one. Variables
    // are still collected in SOURCE order so the projection header does not
    // depend on the plan.
    let mut all_vars: Vec<String> = Vec::new();
    for tp in patterns {
        for var in triple_pattern_vars(tp) {
            if !all_vars.contains(&var) {
                all_vars.push(var);
            }
        }
    }

    // The one graph this BGP is scoped to — the applicability guard admits
    // only single-graph scopes, so the fallback is unreachable in practice.
    let graph = ctx
        .graph
        .single_graph()
        .unwrap_or(crate::schema::ROOT_GRAPH);

    // Plan on index-length estimates: they cost no value
    // decode and are exact for every access path but one, so the fold order
    // is the one measured row sets would give. Materializing every pattern up
    // front to measure it was the cost: inside a correlated NOT EXISTS a
    // pattern sharing no variable with the outer row (`?blocker status ?s`)
    // materialized every fact of its predicate once PER OUTER ROW.
    let mut estimates: Vec<usize> = Vec::with_capacity(patterns.len());
    for (i, tp) in patterns.iter().enumerate() {
        check_eval_budget(ctx, i, 0)?;
        let estimate = estimate_triple_pattern_from_model(store, tp, seed, graph)?;
        // Zero is exact: an empty pattern empties every join it is in.
        if estimate == 0 {
            return Ok((Vec::new(), all_vars));
        }
        estimates.push(estimate);
    }

    let pattern_vars: Vec<Vec<String>> = patterns.iter().map(triple_pattern_vars).collect();
    let bound: HashSet<String> = seed.keys().cloned().collect();
    let plan = join_plan(&pattern_vars, &estimates, &bound);

    let mut result_rows: Vec<Bindings> = vec![seed.clone()];
    for (step, idx) in plan.into_iter().enumerate() {
        check_eval_budget(ctx, step, result_rows.len())?;
        let shares_bound = result_rows[0].keys().any(|k| pattern_vars[idx].contains(k));
        result_rows = if shares_bound && result_rows.len() < estimates[idx] {
            // Fewer accumulated rows than the pattern has facts: probe the
            // model once per row with the shared variables bound, an index
            // hit each, instead of materializing the whole pattern to join.
            bind_join(store, &patterns[idx], &result_rows, graph, ctx)?
        } else {
            let rows = eval_triple_pattern_from_model(store, &patterns[idx], seed, graph)?;
            hash_join_bindings(&result_rows, &rows, ctx)?
        };
        // An empty intermediate can never grow again, so stop rather than
        // joining the remaining patterns for nothing.
        if result_rows.is_empty() {
            break;
        }
    }
    // The BGP is the whole prefix-safe subtree here, so a pushed-down LIMIT
    // may truncate the final solution set (never an intermediate — every row
    // above could still have survived or multiplied through later joins).
    if let Some(cap) = ctx.row_limit {
        result_rows.truncate(cap);
    }

    Ok((result_rows, all_vars))
}

/// Extend each row by the pattern evaluated with that row's bindings: the
/// nested loop, but over model index lookups, and only for the step where it
/// is cheaper than a hash join. [`super::triple_bind::bind_row`] carries each
/// row's bindings through, so the output rows are already merged.
fn bind_join(
    store: &Store,
    tp: &TriplePattern,
    rows: &[Bindings],
    graph: i64,
    ctx: &TemporalContext,
) -> Result<Vec<Bindings>> {
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        check_eval_budget(ctx, i, out.len())?;
        out.extend(eval_triple_pattern_from_model(store, tp, row, graph)?);
    }
    Ok(out)
}

/// Choose the hash-join fold order (quipu-0lr): smallest row set
/// first, then greedily the smallest pattern CONNECTED to a variable already
/// bound, falling back to the smallest disconnected pattern only when nothing
/// connects (that cartesian is then genuinely in the query). Ties break on
/// source index, so the plan is deterministic.
///
/// Pure over (per-pattern variables, per-pattern cardinalities, initially
/// bound variables) exactly so the acceptance is testable: a pathological
/// source ordering must produce the same plan as a good one.
pub(crate) fn join_plan(
    pattern_vars: &[Vec<String>],
    cardinalities: &[usize],
    initially_bound: &HashSet<String>,
) -> Vec<usize> {
    let n = cardinalities.len();
    let mut bound: HashSet<String> = initially_bound.clone();
    let mut remaining: Vec<usize> = (0..n).collect();
    let mut order = Vec::with_capacity(n);
    while !remaining.is_empty() {
        let connected = |i: &usize| pattern_vars[*i].iter().any(|v| bound.contains(v));
        // `order.is_empty()` starts from the smallest pattern outright; the
        // seed's bindings count as connections because those patterns were
        // evaluated under the seed and are already selective.
        let candidates: Vec<usize> = if order.is_empty() && bound.is_empty() {
            remaining.clone()
        } else {
            let conn: Vec<usize> = remaining.iter().filter(|i| connected(i)).copied().collect();
            if conn.is_empty() {
                remaining.clone()
            } else {
                conn
            }
        };
        let pick = candidates
            .into_iter()
            .min_by_key(|&i| (cardinalities[i], i))
            .expect("candidates cannot be empty while remaining is not");
        remaining.retain(|&i| i != pick);
        for v in &pattern_vars[pick] {
            bound.insert(v.clone());
        }
        order.push(pick);
    }
    order
}

/// Join two binding sets on the variables they share.
///
/// With no shared variables this is a cartesian product, which is what the
/// nested loop produced too — an unconnected BGP genuinely has that many
/// solutions. The row cap is what stops it running away.
pub(super) fn hash_join_bindings(
    left: &[Bindings],
    right: &[Bindings],
    ctx: &TemporalContext,
) -> Result<Vec<Bindings>> {
    if left.is_empty() || right.is_empty() {
        return Ok(Vec::new());
    }

    // Every row of a BGP result binds the same variables, so one row of each
    // side is enough to find the join keys.
    let mut shared: Vec<&String> = left[0]
        .keys()
        .filter(|k| right[0].contains_key(*k))
        .collect();
    shared.sort_unstable();

    if shared.is_empty() {
        let mut out = Vec::new();
        for (i, l) in left.iter().enumerate() {
            check_eval_budget(ctx, i, out.len())?;
            for r in right {
                let mut merged = l.clone();
                merged.extend(r.iter().map(|(k, v)| (k.clone(), v.clone())));
                out.push(merged);
            }
        }
        return Ok(out);
    }

    // Build from the smaller side, probe with the larger — the standard choice,
    // and the one that keeps the hash table off the hot path when one side is a
    // whole-store scan and the other is a handful of rows.
    let key_of = |b: &Bindings| -> Vec<Vec<u8>> {
        shared
            .iter()
            .map(|k| b.get(*k).map(Value::term_key).unwrap_or_default())
            .collect()
    };

    let (build, probe, build_is_left) = if right.len() <= left.len() {
        (right, left, false)
    } else {
        (left, right, true)
    };

    let mut table: HashMap<Vec<Vec<u8>>, Vec<&Bindings>> = HashMap::new();
    for b in build {
        table.entry(key_of(b)).or_default().push(b);
    }

    let mut out = Vec::new();
    for (i, p) in probe.iter().enumerate() {
        check_eval_budget(ctx, i, out.len())?;
        let Some(matches) = table.get(&key_of(p)) else {
            continue;
        };
        for m in matches {
            // Keep left-then-right precedence regardless of which side was
            // built, so the merged row is identical either way. The shared keys
            // are equal by construction, so only the non-shared ones matter.
            let (l, r): (&Bindings, &Bindings) = if build_is_left { (m, p) } else { (p, m) };
            let mut merged = l.clone();
            merged.extend(r.iter().map(|(k, v)| (k.clone(), v.clone())));
            out.push(merged);
        }
    }
    Ok(out)
}
