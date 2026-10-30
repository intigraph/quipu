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
//!
//! # Top `k` under ORDER BY LIMIT
//!
//! The same panel orders by `DESC(?end)` and keeps 101 rows. A wide window
//! (11 days kept 33,719 of 72,848 end times on the 2026-10-24 board) both
//! fails the half-the-driver test above, so the original plan probed every
//! done item, and would cost one probe per survivor if driven. When the
//! enclosing `OrderBy` hands down its keys and `k`, the survivors are sorted
//! first and only probed until `k` joined rows exist. The answer is the one the
//! full join gives:
//!
//! - every key reads only variables the driver binds, so each joined row
//!   carries its survivor's keys, and the other patterns never rebind them;
//! - the sort is stable, so the survivors keep their scan order among equal
//!   keys, and the joined rows reach the outer stable top-`k` in the same
//!   relative order the full join would produce. A survivor sorted after the
//!   one that reached `k` has keys no better than those `k` rows and comes
//!   after them in that order, so none of its rows could be returned, even
//!   on a tie;
//! - the whole filter is judged on the driver, and no conjunct raises on any
//!   driver row. Each joined row then meets the caller's filter exactly as its
//!   survivor did, so counting joined rows counts returned rows, and no row
//!   the original would have filtered can hide an error the original raises.
//!   A survivor some conjunct leaves a type error (an ill-typed end time) is
//!   one the caller's filter drops, so only the rows every conjunct judged
//!   `true` are ordered;
//! - the keys must be totally ordered over all survivors, the same
//!   comparison-class test `order_pushdown` applies.
//!
//! Anything else takes the plan above unchanged.

use std::collections::HashSet;

use spargebra::algebra::{Expression, Function, GraphPattern, OrderExpression};
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

/// The ORDER BY keys and limit an `OrderBy` armed for one BGP node.
struct Top {
    bgp: *const GraphPattern,
    keys: Vec<OrderExpression>,
    k: usize,
}

thread_local! {
    static TOP: std::cell::RefCell<Option<Top>> = const { std::cell::RefCell::new(None) };
}

/// Offers an `OrderBy`'s keys and limit to the filtered BGP directly below
/// it, and to no other pattern: [`try_eval`] takes them only for that exact
/// node, so a pattern inside `EXISTS`, a subquery or the COUNT path never
/// sees them. Restores the enclosing offer when dropped.
pub(super) struct TopGuard(Option<Option<Top>>);

impl TopGuard {
    /// Arm for `inner` when it is a `Filter` over a BGP, directly or under
    /// one named graph (which only re-scopes it), and `limit` is set.
    pub(super) fn arm(
        inner: &GraphPattern,
        keys: &[OrderExpression],
        limit: Option<usize>,
    ) -> Self {
        let inner = match inner {
            GraphPattern::Graph {
                name: NamedNodePattern::NamedNode(_),
                inner,
            } => inner.as_ref(),
            other => other,
        };
        let (Some(k), GraphPattern::Filter { inner: bgp, .. }) = (limit, inner) else {
            return Self(None);
        };
        if !matches!(bgp.as_ref(), GraphPattern::Bgp { .. }) {
            return Self(None);
        }
        let top = Top {
            bgp: std::ptr::from_ref(bgp.as_ref()),
            keys: keys.to_vec(),
            k,
        };
        Self(Some(TOP.with(|t| t.replace(Some(top)))))
    }
}

impl Drop for TopGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.0.take() {
            TOP.with(|t| *t.borrow_mut() = previous);
        }
    }
}

/// Rows of the BGP under `seed`, driven from a filtered pattern, or `None`
/// when the original evaluation should run. The rows are not yet filtered by
/// the whole expression. Under an armed [`TopGuard`] for `inner`, the rows may
/// be only those its ordering can return (see the module docs).
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
    // The floor below is never under MIN_DRIVER_ROWS, so a first pattern that
    // cannot exceed it always declines. Prove that with a bounded probe BEFORE
    // counting the filtered predicate: under `NOT EXISTS` this runs once per
    // outer row with the first pattern seeded, and counting first cost a full
    // predicate count per row only to decline (aegis-xacr.1: 5,117 counts,
    // 8.3 of 8.8 s, for the seeds ready query).
    if drive != 0
        && eval_triple_pattern_limited(store, &patterns[0], seed, ctx, Some(MIN_DRIVER_ROWS + 1))?
            .len()
            <= MIN_DRIVER_ROWS
    {
        return Ok(None);
    }
    #[cfg(test)]
    tests::estimated();
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
    // The kept rows every conjunct judged `true`; the others left the joined
    // rows to decide.
    let mut sure = Vec::new();
    let mut clean = true;
    for (i, row) in rows.into_iter().enumerate() {
        check_eval_budget(ctx, i, kept.len())?;
        let verdict = keeps(store, &judged, &row, ctx)?;
        match verdict {
            Verdict::Drop => continue,
            Verdict::Deferred => clean = false,
            Verdict::Keep | Verdict::Unknown => {}
        }
        sure.push(matches!(verdict, Verdict::Keep));
        kept.push(row);
    }
    let rest: Vec<TriplePattern> = patterns
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != drive)
        .map(|(_, p)| p.clone())
        .collect();
    let top = TOP.with(|t| {
        t.borrow()
            .as_ref()
            .filter(|top| std::ptr::eq(top.bgp, inner))
            .map(|top| (top.keys.clone(), top.k))
    });
    #[cfg(test)]
    let top = top.filter(|_| !tests::top_disabled());
    let top = top.filter(|(keys, _)| {
        clean && judged.len() == conjuncts.len() && driver_keys(keys, &patterns[drive], seed)
    });
    // With the whole filter judged and no conjunct raising, a row any
    // conjunct left unknown is one the caller's filter drops: its joined rows
    // see the same values, and an unknown conjunct makes the filter `false`
    // or a type error, never `true`. Only the sure rows can be returned.
    // Both passes below visit every survivor, so both keep the query budget.
    let ordered = match top {
        Some((keys, k)) => {
            let survivors = sure_rows(&kept, &sure, ctx)?;
            super::order_pushdown::total_within(store, &survivors, &keys, ctx)?
                .then_some((survivors, keys, k))
        }
        None => None,
    };
    let joined = if let Some((survivors, keys, k)) = ordered {
        // Probing only about `k` survivors costs less than the original plan
        // however many the filter keeps, so the half-the-driver test is moot.
        #[cfg(test)]
        tests::topped();
        top_joined(store, &rest, ctx, &survivors, &keys, k)?
    } else {
        if drive != 0 && kept.len() > original / 2 {
            return Ok(None);
        }
        let mut joined = Vec::new();
        for (i, row) in kept.iter().enumerate() {
            check_eval_budget(ctx, i, joined.len())?;
            let (rows, _) = eval_bgp(store, &rest, ctx, row)?;
            joined.extend(rows);
        }
        joined
    };
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

/// How the judged prefix decides one driver row.
enum Verdict {
    /// Every conjunct is `true`.
    Keep,
    /// A conjunct is `false`.
    Drop,
    /// Kept: a conjunct's value is a type error, none is `false`, and none
    /// raises.
    Unknown,
    /// Kept for the joined rows to decide: a conjunct raises before any is
    /// `false`.
    Deferred,
}

/// Whether a driver row survives the judged prefix, as the original would
/// decide it on every row joined from it. See the module docs.
fn keeps(
    store: &Store,
    prefix: &[&Expression],
    row: &Bindings,
    ctx: &TemporalContext,
) -> Result<Verdict> {
    let mut verdict = Verdict::Keep;
    for conjunct in prefix {
        match eval_filter_tv(store, conjunct, row, ctx) {
            Ok(Some(false)) => return Ok(Verdict::Drop),
            Ok(Some(true)) => {}
            Ok(None) => verdict = Verdict::Unknown,
            Err(error) if budget(&error) => return Err(error),
            // Deferred to the joined rows, where the original raises it.
            Err(_) => return Ok(Verdict::Deferred),
        }
    }
    Ok(verdict)
}

/// Whether every ORDER BY key reads only variables the driver (or the seed)
/// binds, and nothing a row could change after the driver.
fn driver_keys(keys: &[OrderExpression], driver: &TriplePattern, seed: &Bindings) -> bool {
    let bound: HashSet<String> = triple_pattern_vars(driver)
        .into_iter()
        .chain(seed.keys().cloned())
        .collect();
    keys.iter().all(|order| {
        let (OrderExpression::Asc(e) | OrderExpression::Desc(e)) = order;
        let mut vars = HashSet::new();
        stable_vars(e, &mut vars)
            && vars.is_subset(&bound)
            && !vars
                .iter()
                .any(|v| super::order_pushdown::METADATA.contains(&v.as_str()))
    })
}

/// The kept rows every conjunct judged `true`, under the query budget: this
/// visits every survivor, up to the driver's scan ceiling.
fn sure_rows(kept: &[Bindings], sure: &[bool], ctx: &TemporalContext) -> Result<Vec<Bindings>> {
    let mut rows = Vec::new();
    for (i, (row, sure)) in kept.iter().zip(sure).enumerate() {
        check_eval_budget(ctx, i, rows.len())?;
        if *sure {
            rows.push(row.clone());
        }
    }
    Ok(rows)
}

/// Above this many ordered survivors, probe them all in scan order instead.
const MAX_TOP_SURVIVORS: usize = 4096;

/// The joined rows of the survivors an ORDER BY `keys` LIMIT `k` can return:
/// survivors in stable key order, probed until `k` rows exist.
///
/// A survivor after the one that reached `k` has keys no better than those
/// `k` rows and reaches the outer stable top-`k` after them, so none of its
/// rows can be returned, tied or not.
///
/// The survivors are ordered by the bounded stable top-`m` the `OrderBy`
/// itself uses, doubling `m` when its prefix runs out. A stable top-`m` is a
/// prefix of the stable sort, so probing resumes where the shorter one ended.
fn top_joined(
    store: &Store,
    rest: &[TriplePattern],
    ctx: &TemporalContext,
    kept: &[Bindings],
    keys: &[OrderExpression],
    k: usize,
) -> Result<Vec<Bindings>> {
    let mut joined = Vec::new();
    let mut probed = 0;
    let mut m = k.max(1);
    loop {
        if m >= kept.len().min(MAX_TOP_SURVIVORS + 1) && m < kept.len() {
            // Too many survivors fail the join: finish in scan order.
            return all_joined(store, rest, ctx, kept);
        }
        let prefix = super::order::top_rows(store, kept.to_vec(), keys, ctx, m)?;
        for row in &prefix[probed..] {
            if joined.len() >= k {
                return Ok(joined);
            }
            check_eval_budget(ctx, probed, joined.len())?;
            #[cfg(test)]
            tests::probed();
            let (rows, _) = eval_bgp(store, rest, ctx, row)?;
            joined.extend(rows);
            probed += 1;
        }
        if joined.len() >= k || prefix.len() < m {
            // Enough rows, or every survivor is probed.
            return Ok(joined);
        }
        m = m.saturating_mul(2);
    }
}

/// Every survivor's joined rows, in scan order.
fn all_joined(
    store: &Store,
    rest: &[TriplePattern],
    ctx: &TemporalContext,
    kept: &[Bindings],
) -> Result<Vec<Bindings>> {
    let mut joined = Vec::new();
    for (i, row) in kept.iter().enumerate() {
        check_eval_budget(ctx, i, joined.len())?;
        #[cfg(test)]
        tests::probed();
        let (rows, _) = eval_bgp(store, rest, ctx, row)?;
        joined.extend(rows);
    }
    Ok(joined)
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
            // spargebra represents our implemented XSD casts as Custom too.
            // Rejecting them makes date-bound filters hydrate the whole BGP.
            let stable = match function {
                Function::Custom(name) => super::casts::is_cast(name.as_str()),
                Function::Rand
                | Function::Now
                | Function::Uuid
                | Function::StrUuid
                | Function::BNode => false,
                _ => true,
            };
            stable && args.iter().all(|e| stable_vars(e, vars))
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

#[cfg(test)]
#[path = "filter_drive_top_tests.rs"]
mod top_tests;
