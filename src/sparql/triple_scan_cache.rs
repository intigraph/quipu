//! Per-row scan statements come from the connection's statement cache
//! (aegis-ek4).
//!
//! A nested-loop BGP step or correlated EXISTS issues the same scan SQL once
//! per outer row; only the bound parameters change. Parsing and planning that
//! text again for every row was most of a board GROUP BY's time.
//!
//! The cache hands out one statement per SQL text, and a scan that is still
//! stepping keeps its statement checked out while its rows drive the inner
//! patterns. `?s :identifier ?id . ?s :status ?st` issues the SAME text for
//! both inner steps (the predicate is a parameter), so the inner one would
//! miss every row and prepare afresh. Keying by how many scans are open
//! around this one gives each nesting level its own cached statement.

use std::cell::Cell;

use crate::Result;
use crate::store::Store;

thread_local! {
    static OPEN_SCANS: Cell<usize> = const { Cell::new(0) };
}

/// Marks one open scan for as long as its statement is checked out.
pub(super) struct Open;

impl Drop for Open {
    fn drop(&mut self) {
        OPEN_SCANS.with(|open| open.set(open.get().saturating_sub(1)));
    }
}

/// Prepare `sql` through the statement cache, keyed by nesting depth. Hold the
/// returned [`Open`] in the same scope as the statement.
pub(super) fn prepare<'a>(
    store: &'a Store,
    sql: &str,
) -> Result<(rusqlite::CachedStatement<'a>, Open)> {
    let depth = OPEN_SCANS.with(|open| {
        let depth = open.get();
        open.set(depth + 1);
        depth
    });
    let open = Open;
    // A leading comment is whitespace to SQLite and part of the cache key.
    let stmt = if depth == 0 {
        store.prepare_cached(sql)?
    } else {
        store.prepare_cached(&format!("/* scan depth {depth} */ {sql}"))?
    };
    Ok((stmt, open))
}

#[cfg(test)]
#[path = "scan_cache_tests.rs"]
mod tests;
