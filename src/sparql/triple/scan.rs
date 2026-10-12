//! Collection modes for the shared triple visitor.
use super::{
    Bindings, Result, Store, TemporalContext, TriplePattern, visit_triple_pattern_limited,
};

/// A bounded complete relation for SQL-star's speculative composed plan.
/// Rust canonical deduplication already removes every physical duplicate;
/// avoid sorting the physical relation again before that pass. An overflow
/// restores the ordinary planner, so this order never selects a result prefix.
pub(in crate::sparql) fn eval_complete_relation(
    store: &Store,
    tp: &TriplePattern,
    ctx: &TemporalContext,
    limit: usize,
) -> Result<Vec<Bindings>> {
    collect_triple_pattern(
        store,
        tp,
        &Bindings::new(),
        ctx,
        Some(limit),
        ScanMode::CompleteRelation,
    )
}

#[derive(Clone, Copy)]
pub(super) enum ScanMode {
    Retained,
    Streamed,
    CompleteRelation,
}

pub(super) fn collect_triple_pattern(
    store: &Store,
    tp: &TriplePattern,
    bindings: &Bindings,
    ctx: &TemporalContext,
    limit: Option<usize>,
    mode: ScanMode,
) -> Result<Vec<Bindings>> {
    let mut results = Vec::new();
    visit_triple_pattern_limited(
        store,
        tp,
        bindings,
        ctx,
        limit,
        mode,
        Some(&mut |row| {
            results.push(row);
            Ok(())
        }),
    )?;
    Ok(results)
}
