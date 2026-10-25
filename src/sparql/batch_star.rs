//! Batch a dense selected subject star through the ordinary canonical leaf.
use super::pattern_util::{check_eval_budget, triple_pattern_vars};
use super::triple::{binding_bytes, selected_relation, subject_relation};
use super::{Bindings, TemporalContext};
use crate::{Result, Store, Value};
use spargebra::term::{NamedNodePattern, TermPattern, TriplePattern};
use std::collections::HashSet;

const MAX_SUBJECTS: usize = 16_384;
const MAX_ROWS: usize = 50_000;
pub(super) const MAX_BYTES: usize = 8 * 1024 * 1024;
/// Subjects per window of a dense anchor. [`binding_bytes`] charges about
/// 1 KiB for a one- or two-variable row, so a window's relations stay within
/// [`MAX_BYTES`] at up to two rows per subject.
const WINDOW_SUBJECTS: usize = 4_096;
/// The anchor binds only the subject, so [`MAX_ROWS`] of its rows account to
/// about 51 MiB. The plan this replaces retains the same rows without a bound.
const ANCHOR_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn try_eval(
    store: &Store,
    patterns: &[TriplePattern],
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    try_eval_windowed(store, patterns, ctx, seed, WINDOW_SUBJECTS)
}

/// [`try_eval`] with an explicit window, so tests can split a small star.
pub(super) fn try_eval_windowed(
    store: &Store,
    patterns: &[TriplePattern],
    ctx: &TemporalContext,
    seed: &Bindings,
    window: usize,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    #[cfg(test)]
    if super::filter_join::reference_disabled() {
        return Ok(None);
    }
    if !(2..=8).contains(&patterns.len())
        || !patterns.iter().any(|p| matches!(&p.predicate, NamedNodePattern::NamedNode(n) if n.as_str() == crate::namespace::RDF_TYPE))
        || !seed.is_empty()
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
        || ctx.entails_rdfs
        || ctx.row_limit.is_some()
        || ctx.string_narrows.is_some()
        || ctx.row_cap.is_some_and(|cap| cap < MAX_ROWS + 1)
        || !matches!(&ctx.graph, super::GraphScope::Named(ids) if !ids.is_empty() && ids.iter().all(|id| *id > 0))
        || !matches!(patterns[0].subject, TermPattern::Variable(_))
        || patterns.iter().any(|p| {
            p.subject != patterns[0].subject
                || !matches!(p.predicate, NamedNodePattern::NamedNode(_))
                || matches!(p.object, TermPattern::BlankNode(_))
        })
    {
        return Ok(None);
    }
    let Some(anchor) = patterns.iter().position(|p| {
        matches!(
            p.object,
            TermPattern::Literal(_) | TermPattern::NamedNode(_)
        ) && !matches!(&p.predicate, NamedNodePattern::NamedNode(n)
            if n.as_str() == crate::namespace::RDF_TYPE)
    }) else {
        return Ok(None);
    };
    // An owner denser than one window (aegis-t9n: 22,679 Actions for one
    // principal) is evaluated window by window instead of once per subject.
    let Some(selected) = speculative(selected_relation(
        store,
        &patterns[anchor],
        ctx,
        MAX_ROWS + 1,
        ANCHOR_BYTES,
    ))?
    else {
        return Ok(None);
    };
    // Small stars retain the proven sparse plan. Overflow never returns a prefix.
    if selected.len() <= super::sparse_bgp::SMALL || selected.len() > MAX_ROWS {
        return Ok(None);
    }
    evaluate_windows(store, patterns, ctx, &selected, anchor, window)
}

/// Evaluate the star over consecutive windows of the anchor's rows.
///
/// Every pattern shares the one subject variable, so the star over all anchor
/// rows is the concatenation of the star over each window of them, in anchor
/// order. Each window keeps the per-relation row and byte bounds. A window
/// that declines restores the original planner for the whole BGP: no prefix
/// is returned.
fn evaluate_windows(
    store: &Store,
    patterns: &[TriplePattern],
    ctx: &TemporalContext,
    selected: &[Bindings],
    anchor: usize,
    window: usize,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    let mut rows = Vec::new();
    let mut variables = Vec::new();
    for chunk in selected.chunks(window) {
        let Some((found, vars)) =
            evaluate_selected(store, patterns, ctx, chunk.to_vec(), Some(anchor))?
        else {
            return Ok(None);
        };
        rows.extend(found);
        // Index 0 polls the deadline after every window; the cap sees the total.
        check_eval_budget(ctx, 0, rows.len())?;
        variables = vars;
    }
    Ok(Some((rows, variables)))
}

/// Reuse a subject-only driving relation already proven by the guarded join.
pub(super) fn from_selected(
    store: &Store,
    patterns: &[TriplePattern],
    ctx: &TemporalContext,
    selected: &[Bindings],
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    if !(1..=8).contains(&patterns.len())
        || selected.len() > MAX_SUBJECTS
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
        || ctx.entails_rdfs
        || ctx.row_limit.is_some()
        || ctx.string_narrows.is_some()
        || ctx.row_cap.is_some_and(|cap| cap < MAX_ROWS + 1)
        || !matches!(&ctx.graph, super::GraphScope::Named(ids) if !ids.is_empty() && ids.iter().all(|id| *id > 0))
        || !matches!(patterns[0].subject, TermPattern::Variable(_))
        || patterns.iter().any(|p| {
            p.subject != patterns[0].subject
                || !matches!(p.predicate, NamedNodePattern::NamedNode(_))
                || matches!(p.object, TermPattern::BlankNode(_))
        })
    {
        return Ok(None);
    }
    let TermPattern::Variable(subject) = &patterns[0].subject else {
        unreachable!()
    };
    let mut bytes = 0usize;
    for (i, row) in selected.iter().enumerate() {
        check_eval_budget(ctx, i, 0)?;
        if row.len() != 1 || !matches!(row.get(subject.as_str()), Some(Value::Ref(id)) if *id > 0) {
            return Ok(None);
        }
        bytes = bytes.saturating_add(binding_bytes(row));
        if bytes > MAX_BYTES {
            return Ok(None);
        }
    }
    evaluate_selected(store, patterns, ctx, selected.to_vec(), None)
}

fn evaluate_selected(
    store: &Store,
    patterns: &[TriplePattern],
    ctx: &TemporalContext,
    selected: Vec<Bindings>,
    skip: Option<usize>,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    let TermPattern::Variable(subject) = &patterns[0].subject else {
        unreachable!()
    };
    let mut seen = HashSet::new();
    let mut groups = Vec::new();
    for (i, row) in selected.iter().enumerate() {
        check_eval_budget(ctx, i, 0)?;
        let Some(Value::Ref(id)) = row.get(subject.as_str()) else {
            return Ok(None);
        };
        if *id <= 0 {
            return Ok(None);
        }
        if !seen.insert(*id) {
            continue;
        }
        let aliases = store.lookup_all(&store.resolve(*id)?)?;
        if aliases.is_empty() || aliases.len() > 128 {
            return Ok(None);
        }
        groups.push(aliases);
    }
    let mut variables = Vec::new();
    for p in patterns {
        for v in triple_pattern_vars(p) {
            if !variables.contains(&v) {
                variables.push(v);
            }
        }
    }
    let mut rows = selected;
    for (index, pattern) in patterns.iter().enumerate() {
        if skip == Some(index) {
            continue;
        }
        let mut relation = Vec::new();
        let mut bytes = 0usize;
        for (step, chunk) in groups.chunks(128).enumerate() {
            check_eval_budget(ctx, step, relation.len())?;
            let ids: Vec<_> = chunk.iter().flatten().copied().collect();
            let remaining = MAX_ROWS + 1 - relation.len();
            let Some(found) = speculative(subject_relation(
                store,
                pattern,
                ctx,
                &ids,
                remaining,
                MAX_BYTES - bytes,
            ))?
            else {
                return Ok(None);
            };
            bytes += found.iter().map(binding_bytes).sum::<usize>();
            relation.extend(found);
            if relation.len() > MAX_ROWS {
                return Ok(None);
            }
        }
        rows = super::join::hash_join_bindings(&rows, &relation, ctx)?;
        if rows.is_empty() {
            break;
        }
    }
    Ok(Some((rows, variables)))
}

// Speculation can see a malformed row that the ordinary source-order plan
// would eliminate earlier. Only cancellation/budget errors must propagate.
pub(super) fn speculative(result: Result<Vec<Bindings>>) -> Result<Option<Vec<Bindings>>> {
    match result {
        Ok(rows) => Ok(Some(rows)),
        Err(crate::Error::InvalidValue(_) | crate::Error::UnknownTerm(_)) => Ok(None),
        Err(crate::Error::Sqlite(error))
            if !matches!(&error,
            rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::OperationInterrupted) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
