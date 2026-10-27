//! Commit-time ownership fence for a writable handle that is already open.
//!
//! Opening checks ownership once. A handle opened earlier keeps its writable
//! connection, so a later failed adoption (sticky pending marker) or a lost or
//! invalid ENFORCED binding must still refuse that handle's writes. Entrypoint
//! checks (`Store::ensure_ownership_writable`) give a clear error before any
//! audit row, but they cannot cover every direct SQL writer. This fence does:
//! SQLite calls it at every outer COMMIT on the connection, including
//! autocommit statements and outermost RELEASE, while this connection already
//! holds the write lock. Returning `true` turns the commit into a rollback, so
//! no refused write lands and no adoption can race in between check and commit.
//!
//! The hook may not run SQL on the committing connection, so the check runs on
//! a second, read-only connection to the same file. In WAL mode that reader is
//! not blocked by the writer and sees the latest committed ownership state
//! (adoption commits its marker on its own connection). Any failure to open or
//! query that reader refuses the commit: the fence fails closed.
//!
//! A refusal surfaces as SQLite's "commit hook" constraint error, since the
//! hook has no channel for its own message; entrypoint checks keep the
//! descriptive ownership error for the common write paths.
//!
//! SQLite rolls the data back on a refused commit, but a refused outermost
//! RELEASE leaves the connection marked as inside a transaction. The hook
//! cannot issue SQL, so every write scope ends through `store::release`,
//! which rolls back at once and reports the refusal.
use rusqlite::{Connection, OpenFlags};

/// The fence for the store file at `path`, or `None` for in-memory and
/// browser stores (INACTIVE by definition, with no native placement).
pub(super) fn commit_fence(conn: &Connection) -> Option<impl FnMut() -> bool + Send + 'static> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let path = conn.path().filter(|p| !p.is_empty())?.to_owned();
    // A store file unlinked while open (a dropped temporary file, say) cannot
    // be opened again. Nothing can adopt or bind a path that no longer exists,
    // so such a handle keeps the state it opened with: INACTIVE stays
    // writable, and ENFORCED (whose binding needs the path) refuses.
    let opened_inactive = matches!(
        super::ownership::state_on(conn),
        Ok(super::OwnershipState::Inactive)
    );
    let mut reader: Option<Connection> = None;
    Some(move || {
        if reader.is_none() {
            reader = Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .and_then(|c| c.execute_batch("PRAGMA query_only=ON;").map(|()| c))
            .ok();
        }
        match reader.as_ref() {
            Some(reader) => super::ownership::state_on(reader).is_err(),
            None => {
                let gone = std::fs::symlink_metadata(&path)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
                !(gone && opened_inactive)
            }
        }
    })
}
