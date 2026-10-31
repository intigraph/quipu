//! Fail-closed reservation for the handoff enrollment boundary.
//!
//! Stage-A native adoption is not handoff enrollment. Until the protected
//! journal and authenticated, nonce-bound external startup adapter exist, an
//! enrolled database has no writable startup path. Shape/model validation is
//! deliberately never accepted as authority here.
use rusqlite::Connection;

use crate::error::{Error, Result};

/// Presence is sticky enrollment evidence even if the object is empty, a view,
/// malformed, or from a newer version. Do not interpret any of those as legacy.
pub(super) fn enrolled(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_master
         WHERE name='ownership_handoff_enrollment' COLLATE NOCASE)",
        [],
        |row| row.get(0),
    )
}

pub(super) fn require_unenrolled(conn: &Connection) -> Result<()> {
    if enrolled(conn)? {
        return Err(unavailable());
    }
    Ok(())
}

pub(super) fn unavailable() -> Error {
    Error::PolicyDenied(
        "store handoff: enrolled startup requires a protected journal and fresh authenticated external admission; writable startup is unavailable".into(),
    )
}
