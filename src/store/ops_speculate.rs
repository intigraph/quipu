//! Speculative writes that are always rolled back. Split from `ops.rs`.

use crate::error::Result;

use super::super::{Datum, Store};

impl Store {
    /// Execute `f` against a speculative fork of the store with `hypothetical`
    /// datums applied. The fork is discarded after `f` returns — the underlying
    /// store is never mutated. Useful for counterfactual impact analysis
    /// ("what would break if I removed this?").
    ///
    /// Implementation: wraps the hypothetical write and query in a `SQLite`
    /// `SAVEPOINT` that is always rolled back. Because [`transact`](Store::transact)
    /// uses nested savepoints, observers (including the reactive reasoner) fire
    /// normally inside the speculative fork, so derived facts are recomputed.
    pub fn speculate<F, R>(&mut self, hypothetical: &[Datum], timestamp: &str, f: F) -> Result<R>
    where
        F: FnOnce(&Store) -> Result<R>,
    {
        self.conn.execute_batch("SAVEPOINT speculate")?;

        let result = self
            .transact(
                hypothetical,
                timestamp,
                Some("speculate"),
                Some("speculate"),
            )
            .and_then(|_| f(self));

        // Always rollback: speculative state must not persist.
        match crate::store::release::rollback_release(&self.conn, "speculate") {
            Ok(()) => result,
            Err(rollback_err) => match result {
                Err(e) => Err(e),
                Ok(_) => Err(rollback_err.into()),
            },
        }
    }
}
