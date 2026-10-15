//! OPTIONAL (`LeftJoin`) evaluation: a keyed probe instead of an L x R scan
//! (aegis-d8c).
//!
//! The nested loop compared every left row with every right row. For an
//! OPTIONAL over a whole type (`?s a T . OPTIONAL { ?s p ?o }`) that is
//! quadratic in the subject count, and a board query with ten such OPTIONALs
//! over ~49k subjects never finished inside the request deadline.
//!
//! Here the right rows are bucketed by the variables they ALL bind, and each
//! left row is compared only with the rows in its bucket. The decision for a
//! pair is unchanged: `merge_bindings` then the OPTIONAL expression, exactly as
//! before. The bucket only narrows which pairs are tried, so it must never
//! separate two rows `merge_bindings` would join:
//!
//! * KEY EQUALITY IS COARSER THAN BINDING EQUALITY. Buckets key on
//!   [`Value::term_key`]. `Value`'s `PartialEq` (`literal_identity.rs`) is true
//!   only on branches that compare either raw fields `term_key` is a
//!   deterministic function of, or `term_key` itself, so `a == b` implies
//!   `term_key(a) == term_key(b)`. Hashing the raw variant would be too FINE:
//!   `Int(1)` equals `"1"^^xsd:integer` across variants. A coarser key only
//!   admits extra candidates, which `merge_bindings` rejects; a finer one
//!   would lose matches.
//! * Federation labels (`_provider`, `_trust`, `_freshness`) never key:
//!   `merge_bindings` ignores them, so keying on them would split
//!   compatible rows.
//! * The key set is computed from the ROWS, not the pattern: a right side with
//!   its own OPTIONAL or UNION can leave a pattern variable unbound in some
//!   rows. A variable keys only if every right row binds it and at least one
//!   left row does. No such variable: the nested loop runs unchanged.
//! * A left row missing any key variable scans every right row, in order.
//! * ORDER IS PRESERVED. Left rows are visited in order and each bucket keeps
//!   its right rows in their original order, so the output sequence equals the
//!   nested loop's, and an unordered LIMIT/OFFSET above returns the same rows.
//! * The evaluation budget is polled per bucketed right row and per tried
//!   pair, so the deadline and the row cap still trip on real work.

use std::cell::Cell;
use std::collections::HashMap;

use spargebra::algebra::Expression;

use crate::error::Result;
use crate::store::Store;

use super::filter::eval_filter;
use super::pattern_util::{check_eval_budget, merge_bindings};
use super::{Bindings, TemporalContext};

/// Variables `merge_bindings` treats as response metadata, never as join keys.
const NON_KEY: [&str; 3] = ["_provider", "_trust", "_freshness"];

thread_local! {
    /// Left rows that took the full-scan fallback, and left rows that probed a
    /// bucket, on this thread. Read by tests and the d8c fixture so a future
    /// regression to "mostly fallback" is visible (aegis-d8c condition 6).
    static FALLBACK_ROWS: Cell<usize> = const { Cell::new(0) };
    static PROBED_ROWS: Cell<usize> = const { Cell::new(0) };
}

/// `(fallback_rows, probed_rows)` counted on this thread since the last reset.
#[must_use]
pub fn probe_counts() -> (usize, usize) {
    (FALLBACK_ROWS.with(Cell::get), PROBED_ROWS.with(Cell::get))
}

/// Zero this thread's [`probe_counts`].
pub fn reset_probe_counts() {
    FALLBACK_ROWS.with(|c| c.set(0));
    PROBED_ROWS.with(|c| c.set(0));
}

/// Variables bound in every right row and in at least one left row, in a
/// stable (sorted) order. Empty when either side is empty.
pub(crate) fn key_vars(left: &[Bindings], right: &[Bindings]) -> Vec<String> {
    let Some(first) = right.first() else {
        return Vec::new();
    };
    let mut keys: Vec<String> = first
        .keys()
        .filter(|k| !NON_KEY.contains(&k.as_str()))
        .filter(|k| right.iter().all(|r| r.contains_key(k.as_str())))
        .filter(|k| left.iter().any(|l| l.contains_key(k.as_str())))
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn row_key(row: &Bindings, keys: &[String]) -> Option<Vec<Vec<u8>>> {
    keys.iter()
        .map(|k| row.get(k).map(crate::types::Value::term_key))
        .collect()
}

/// Evaluate `left OPTIONAL { right } [FILTER expression]` over materialized
/// rows. Same output, in the same order, as [`left_join_nested`].
pub(crate) fn left_join(
    store: &Store,
    ctx: &TemporalContext,
    left: &[Bindings],
    right: &[Bindings],
    expression: Option<&Expression>,
) -> Result<Vec<Bindings>> {
    let keys = key_vars(left, right);
    if keys.is_empty() {
        return left_join_nested(store, ctx, left, right, expression);
    }
    let mut results = Vec::new();
    let mut i = 0usize;
    let mut buckets: HashMap<Vec<Vec<u8>>, Vec<usize>> = HashMap::new();
    for (index, r) in right.iter().enumerate() {
        check_eval_budget(ctx, i, results.len())?;
        i += 1;
        // Every right row binds every key, by construction of `key_vars`.
        let key = row_key(r, &keys).expect("key variables are bound in every right row");
        buckets.entry(key).or_default().push(index);
    }
    let everything: Vec<usize> = (0..right.len()).collect();
    for l in left {
        let candidates: &[usize] = match row_key(l, &keys) {
            Some(key) => {
                PROBED_ROWS.with(|c| c.set(c.get() + 1));
                buckets.get(&key).map_or(&[], Vec::as_slice)
            }
            None => {
                FALLBACK_ROWS.with(|c| c.set(c.get() + 1));
                &everything
            }
        };
        let mut matched = false;
        for &index in candidates {
            check_eval_budget(ctx, i, results.len())?;
            i += 1;
            if let Some(merged) = merge_bindings(l, &right[index]) {
                let passes = match expression {
                    Some(e) => eval_filter(store, e, &merged, ctx)?,
                    None => true,
                };
                if passes {
                    results.push(merged);
                    matched = true;
                }
            }
        }
        if !matched {
            results.push(l.clone());
        }
    }
    Ok(results)
}

/// The original nested loop, kept as the reference the keyed path must equal
/// and as the path for a join with no usable key.
pub(crate) fn left_join_nested(
    store: &Store,
    ctx: &TemporalContext,
    left: &[Bindings],
    right: &[Bindings],
    expression: Option<&Expression>,
) -> Result<Vec<Bindings>> {
    let mut results = Vec::new();
    // Same nested join loop as `join_rows`, same budget enforcement, for the
    // same reason (OPTIONAL explodes exactly like Join).
    let mut i = 0usize;
    for l in left {
        let mut matched = false;
        for r in right {
            check_eval_budget(ctx, i, results.len())?;
            i += 1;
            if let Some(merged) = merge_bindings(l, r) {
                let passes = match expression {
                    Some(e) => eval_filter(store, e, &merged, ctx)?,
                    None => true,
                };
                if passes {
                    results.push(merged);
                    matched = true;
                }
            }
        }
        if !matched {
            results.push(l.clone());
        }
    }
    Ok(results)
}
