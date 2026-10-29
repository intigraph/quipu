//! Outer write-scope release that never leaves a dead transaction behind.
//!
//! When the ownership commit fence (`ownership_fence`) refuses the commit
//! that an outermost `RELEASE` performs, SQLite rolls the data back but leaves
//! the connection marked as inside a transaction (it keeps the savepoint so a
//! BUSY release could be retried). Until something ends that state, every
//! later statement on the handle runs inside it: a direct write then returns
//! Ok and is silently discarded, and freshly allocated term ids can be handed
//! out again. Every write scope therefore ends through these helpers, which
//! issue `ROLLBACK` at once when the commit was refused and still return the
//! error.
use rusqlite::{Connection, Error, ffi};

/// `RELEASE name`, rolling back immediately if the commit was refused.
pub(crate) fn release(conn: &Connection, name: &str) -> rusqlite::Result<()> {
    settle(conn, conn.execute_batch(&format!("RELEASE {name}")))
}

/// `ROLLBACK TO name; RELEASE name` (a write scope's error path), with the
/// same immediate rollback if even the empty release is refused.
pub(crate) fn rollback_release(conn: &Connection, name: &str) -> rusqlite::Result<()> {
    settle(
        conn,
        conn.execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}")),
    )
}

/// Pass `result` through, first ending the transaction if it is a refused
/// commit that left the connection outside autocommit. Use after any commit
/// form that is not a plain `RELEASE` (a rusqlite savepoint or transaction).
pub(crate) fn settle<T>(conn: &Connection, result: rusqlite::Result<T>) -> rusqlite::Result<T> {
    if let Err(error) = &result
        && refused_commit(error)
        && !conn.is_autocommit()
    {
        // The refused commit already discarded the data; only the open
        // transaction state is left. A failure here leaves the original
        // error as the one reported.
        let _ = conn.execute_batch("ROLLBACK");
    }
    result
}

/// Only the commit hook produces this code, and only on an outer commit, so a
/// rollback is never applied to a caller's still-live outer transaction.
fn refused_commit(error: &Error) -> bool {
    matches!(
        error,
        Error::SqliteFailure(failure, _)
            if failure.extended_code == ffi::SQLITE_CONSTRAINT_COMMITHOOK
    )
}
