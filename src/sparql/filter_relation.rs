//! Complete speculative FILTER-driver relations, with lazy canonical dedup.
//!
//! This scope never selects a query-result prefix: an overflowing relation
//! makes the caller decline the speculative plan. Ordinary leaves and scalar
//! COUNT retain their SQL grouping. No SQL LIMIT or temporal policy changes.
use std::cell::Cell;

use super::{Bindings, TemporalContext};
use crate::{Store, error::Result};
use spargebra::term::TriplePattern;

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn active() -> bool {
    ACTIVE.get()
}

struct Reset(bool);
impl Drop for Reset {
    fn drop(&mut self) {
        ACTIVE.set(self.0);
    }
}

pub(super) fn collect(
    store: &Store,
    pattern: &TriplePattern,
    seed: &Bindings,
    ctx: &TemporalContext,
    limit: usize,
) -> Result<Vec<Bindings>> {
    let _reset = Reset(ACTIVE.replace(true));
    super::triple::eval_triple_pattern_limited(store, pattern, seed, ctx, Some(limit))
}

#[cfg(test)]
mod tests;
