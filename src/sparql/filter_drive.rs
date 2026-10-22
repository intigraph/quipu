//! Drive a filtered BGP from the pattern its FILTER can already judge (aegis-xacr).
//!
//! The Brief's closes panel asks for `?item q:outcome "done" ; s:endTime ?end`
//! with `FILTER(?end >= T)`. The BGP joins first and the filter runs after, so
//! every done item (65,767 on the 2026-10-21 board) was probed for its end
//! time, one SQL statement per row, to keep the 2,299 the filter accepts:
//! 2.7 s on a copy of that board, 4.3 s on the served store.
//!
//! A top-level conjunct whose variables one triple pattern always binds gives
//! the same answer on that pattern's rows as on the joined rows: the other
//! patterns only add bindings for variables it does not read. So the pattern is
//! scanned on its own, the conjunct drops its rows there, and only the
//! survivors probe the rest of the BGP. The caller still applies the whole
//! filter to the joined rows.
//!
//! Dropping a row early must not hide an error the original would raise, and
//! raising one early must not invent an error the original never reaches.
//! `&&` evaluates left to right: a `false` stops it, a type error does not, and
//! a hard error (an invalid `REGEX` pattern, say) fails the query. So:
//!
//! - only a PREFIX of the conjuncts judges early. A conjunct after one the
//!   driver cannot judge might never run in the original, or run after a hard
//!   error that the original raises first;
//! - a `false` in that prefix drops the row, exactly where the original stops;
//! - a hard error KEEPS the row. The original evaluates the filter only on rows
//!   that survive the join, so the error surfaces there, on joined rows, or
//!   not at all. Raising it here failed queries the original answers (review
//!   of #733: an invalid pattern on a subject the join discards);
//! - the query budget is not a property of the row, and still propagates.
//!
//! The rewrite has to beat the plan it replaces, not just be exact. Driving
//! from an unselective pattern can scan far more than a small original driver
//! would have probed, so it is taken only when the original first pattern is
//! itself large (a bounded scan proves at least [`MIN_DRIVER_ROWS`] rows and
//! at least half the filtered pattern's size), the filtered pattern is no
//! larger than [`MAX_SCAN_ROWS`], and the filter keeps at most half as many
//! rows as that proved lower bound. Anything else keeps the original plan.
//!
//! Deliberately excluded: `EXISTS` (it sees the whole row), volatile functions
//! (`RAND`, `NOW`, `UUID`, `STRUUID`, `BNODE`) and custom functions, history
//! and as-of reads, a LIMIT pushed into the BGP, and graphs the resident read
//! model already joins in memory.

use std::collections::HashSet;

use spargebra::algebra::{Expression, Function, GraphPattern};
use spargebra::term::{NamedNodePattern, TermPattern, TriplePattern};

use super::filter::eval_filter_tv;
use super::pattern_util::{check_eval_budget, triple_pattern_vars};
use super::triple::{eval_bgp, eval_triple_pattern_limited};
use super::{Bindings, TemporalContext};
use crate::error::Result;
use crate::store::Store;

/// Below this many rows the original nested loop is already cheap.
pub(super) const MIN_DRIVER_ROWS: usize = 4096;
/// A larger filtered pattern costs more to scan than the probes it can save.
pub(super) const MAX_SCAN_ROWS: usize = 262_144;

/// Rows of the BGP under `seed`, driven from a filtered pattern, or `None`
/// when the original evaluation should run. The rows are not yet filtered by
/// the whole expression.
///
/// # Errors
/// Any error from evaluating a pattern, and the query budget. A conjunct's own
/// error is deferred to the joined rows, where the original plan raises it.
pub(super) fn try_eval(
    store: &Store,
    expr: &Expression,
    inner: &GraphPattern,
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    #[cfg(test)]
    if tests::disabled() {
        return Ok(None);
    }
    let GraphPattern::Bgp { patterns } = inner else {
        return Ok(None);
    };
    if patterns.len() < 2
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
        || ctx.row_limit.is_some()
        || crate::store::read_model::read_model_applicable(store, ctx)
    {
        return Ok(None);
    }
    let mut conjuncts = Vec::new();
    collect_conjuncts(expr, &mut conjuncts);
    let Some((drive, judged)) = choose_driver(patterns, &conjuncts, seed) else {
        return Ok(None);
    };
    let Some(estimate) = estimate_rows(store, &patterns[drive])? else {
        return Ok(None);
    };
    if estimate > MAX_SCAN_ROWS {
        return Ok(None);
    }
    // The original plan probes once per row of its first pattern. Prove that
    // is large before paying for a scan of the filtered pattern.
    let floor = MIN_DRIVER_ROWS.max(estimate / 2);
    let original = if drive == 0 {
        usize::MAX
    } else {
        let rows = eval_triple_pattern_limited(store, &patterns[0], seed, ctx, Some(floor + 1))?;
        if rows.len() <= floor {
            return Ok(None);
        }
        rows.len()
    };
    let rows =
        eval_triple_pattern_limited(store, &patterns[drive], seed, ctx, Some(MAX_SCAN_ROWS + 1))?;
    if rows.len() > MAX_SCAN_ROWS {
        return Ok(None);
    }
    let mut kept = Vec::new();
    for (i, row) in rows.into_iter().enumerate() {
        check_eval_budget(ctx, i, kept.len())?;
        if keeps(store, &judged, &row, ctx)? {
            kept.push(row);
        }
    }
    if drive != 0 && kept.len() > original / 2 {
        return Ok(None);
    }
    let rest: Vec<TriplePattern> = patterns
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != drive)
        .map(|(_, p)| p.clone())
        .collect();
    let mut joined = Vec::new();
    for (i, row) in kept.iter().enumerate() {
        check_eval_budget(ctx, i, joined.len())?;
        let (rows, _) = eval_bgp(store, &rest, ctx, row)?;
        joined.extend(rows);
    }
    // Same header as the BGP evaluator: variables in source pattern order.
    let mut vars = Vec::new();
    for pattern in patterns {
        for var in triple_pattern_vars(pattern) {
            if !vars.contains(&var) {
                vars.push(var);
            }
        }
    }
    #[cfg(test)]
    tests::drove();
    Ok(Some((joined, vars)))
}

/// Whether a driver row survives the judged prefix, as the original would
/// decide it on every row joined from it. See the module docs.
fn keeps(
    store: &Store,
    prefix: &[&Expression],
    row: &Bindings,
    ctx: &TemporalContext,
) -> Result<bool> {
    for conjunct in prefix {
        match eval_filter_tv(store, conjunct, row, ctx) {
            Ok(Some(false)) => return Ok(false),
            Ok(Some(true) | None) => {}
            Err(error) if budget(&error) => return Err(error),
            // Deferred to the joined rows, where the original raises it.
            Err(_) => return Ok(true),
        }
    }
    Ok(true)
}

/// Errors that belong to the query, not to the row being judged.
fn budget(error: &crate::error::Error) -> bool {
    use crate::error::Error;
    matches!(
        error,
        Error::QueryTimeout { .. } | Error::RequestTimeout { .. } | Error::QueryComplexity { .. }
    )
}

fn collect_conjuncts<'a>(expr: &'a Expression, out: &mut Vec<&'a Expression>) {
    if let Expression::And(left, right) = expr {
        collect_conjuncts(left, out);
        collect_conjuncts(right, out);
    } else {
        out.push(expr);
    }
}

/// The first pattern that can judge a leading run of conjuncts alone, with
/// that run (in evaluation order; see the module docs). Only `?s <p> ?o` with
/// a non-type predicate: the shape whose size the estimate can bound and whose
/// scan needs no entailment.
fn choose_driver<'a>(
    patterns: &[TriplePattern],
    conjuncts: &[&'a Expression],
    seed: &Bindings,
) -> Option<(usize, Vec<&'a Expression>)> {
    for (index, pattern) in patterns.iter().enumerate() {
        if !drivable(pattern, seed) {
            continue;
        }
        let bound: HashSet<String> = triple_pattern_vars(pattern)
            .into_iter()
            .chain(seed.keys().cloned())
            .collect();
        let judged: Vec<&Expression> = conjuncts
            .iter()
            .copied()
            .take_while(|conjunct| {
                let mut vars = HashSet::new();
                stable_vars(conjunct, &mut vars) && !vars.is_empty() && vars.is_subset(&bound)
            })
            .collect();
        if !judged.is_empty() {
            return Some((index, judged));
        }
    }
    None
}

fn drivable(pattern: &TriplePattern, seed: &Bindings) -> bool {
    let free = |term: &TermPattern| matches!(term, TermPattern::Variable(v) if !seed.contains_key(v.as_str()));
    free(&pattern.subject)
        && free(&pattern.object)
        && pattern.subject != pattern.object
        && matches!(&pattern.predicate, NamedNodePattern::NamedNode(n)
            if n.as_str() != crate::namespace::RDF_TYPE)
}

/// Collect the variables `expr` reads; `false` if it cannot be judged early.
fn stable_vars(expr: &Expression, vars: &mut HashSet<String>) -> bool {
    match expr {
        Expression::NamedNode(_) | Expression::Literal(_) => true,
        Expression::Variable(v) | Expression::Bound(v) => {
            vars.insert(v.as_str().to_string());
            true
        }
        Expression::Or(a, b)
        | Expression::And(a, b)
        | Expression::Equal(a, b)
        | Expression::SameTerm(a, b)
        | Expression::Greater(a, b)
        | Expression::GreaterOrEqual(a, b)
        | Expression::Less(a, b)
        | Expression::LessOrEqual(a, b)
        | Expression::Add(a, b)
        | Expression::Subtract(a, b)
        | Expression::Multiply(a, b)
        | Expression::Divide(a, b) => stable_vars(a, vars) && stable_vars(b, vars),
        Expression::UnaryPlus(a) | Expression::UnaryMinus(a) | Expression::Not(a) => {
            stable_vars(a, vars)
        }
        Expression::If(a, b, c) => {
            stable_vars(a, vars) && stable_vars(b, vars) && stable_vars(c, vars)
        }
        Expression::In(a, list) => {
            stable_vars(a, vars) && list.iter().all(|e| stable_vars(e, vars))
        }
        Expression::Coalesce(list) => list.iter().all(|e| stable_vars(e, vars)),
        Expression::FunctionCall(function, args) => {
            !matches!(
                function,
                Function::Rand
                    | Function::Now
                    | Function::Uuid
                    | Function::StrUuid
                    | Function::BNode
                    | Function::Custom(_)
            ) && args.iter().all(|e| stable_vars(e, vars))
        }
        Expression::Exists(_) => false,
    }
}

/// Upper bound on the current rows of `?s <p> ?o` in the local store: every
/// graph, read from the covering current-predicate index without touching
/// the fact table. `None` when the index is not the one this assumes.
fn estimate_rows(store: &Store, pattern: &TriplePattern) -> Result<Option<usize>> {
    let NamedNodePattern::NamedNode(predicate) = &pattern.predicate else {
        return Ok(None);
    };
    let Some(id) = store.lookup(predicate.as_str())? else {
        return Ok(Some(0));
    };
    let n: Option<i64> = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM main.facts INDEXED BY idx_current_aev
             WHERE a = ?1 AND op = 1 AND valid_to IS NULL",
            [id],
            |row| row.get(0),
        )
        .ok();
    Ok(n.map(|n| usize::try_from(n).unwrap_or(usize::MAX)))
}

#[cfg(test)]
#[path = "filter_drive_tests.rs"]
mod tests;
