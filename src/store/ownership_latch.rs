//! Per-lineage admission latch (P1, aegis-s24.8): a file lock on a sibling of
//! the protected journal, outside the database unit, so every process on the
//! host that opens the lineage meets the same lock.
//!
//! Lock order, which has no cycle:
//! - retirement: latch EXCLUSIVE, then the SQLite writer lock (`BEGIN
//!   IMMEDIATE`, which waits out any in-flight commit), seal the cut, write
//!   RETIRING, COMMIT;
//! - writers: latch SHARED, then the SQLite writer lock, then the commit hook.
//!
//! No holder of the SQLite writer lock ever waits on the latch: the commit
//! hook only tries it and refuses on contention. A direct SQL writer that
//! passed the hook before retirement took the latch commits before
//! retirement's `BEGIN IMMEDIATE` can start, so its commit is inside the
//! sealed cut, never after it.
//!
//! Holding the latch until SQLite's WAL hook was rejected: rusqlite's
//! `wal_hook` takes a plain function pointer, and setting any WAL hook
//! replaces `wal_autocheckpoint`, which the store sets on purpose.
// Writers' blocking SHARED form and retirement's EXCLUSIVE form have no
// production caller yet: entrypoint guards and retirement are later slices.
#![cfg_attr(not(test), allow(dead_code))]
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

fn refused(why: impl Into<String>) -> Error {
    Error::PolicyDenied(format!("ownership latch: {}", why.into()))
}

/// The latch file for the journal at `journal`.
pub(super) fn path_for(journal: &Path) -> PathBuf {
    let mut name = journal.as_os_str().to_owned();
    name.push(".latch");
    PathBuf::from(name)
}

/// A held latch; dropping it releases the lock.
#[derive(Debug)]
pub(super) struct Held {
    _file: File,
}

fn open(path: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| refused(format!("unavailable: {e}")))
}

/// Shared admission, waiting for any retirement to finish. Never call this
/// while holding the SQLite writer lock.
pub(super) fn shared(path: &Path) -> Result<Held> {
    let file = open(path)?;
    file.lock_shared()
        .map_err(|e| refused(format!("unavailable: {e}")))?;
    Ok(Held { _file: file })
}

/// Shared admission without waiting: the commit hook's form. Contention
/// means retirement holds the latch, and refuses.
pub(super) fn try_shared(path: &Path) -> Result<Held> {
    let file = open(path)?;
    match file.try_lock_shared() {
        Ok(()) => Ok(Held { _file: file }),
        Err(TryLockError::WouldBlock) => Err(refused("held by retirement")),
        Err(TryLockError::Error(e)) => Err(refused(format!("unavailable: {e}"))),
    }
}

/// Exclusive admission for retirement, waiting for admitted writers to
/// finish. Take it BEFORE the SQLite writer lock.
pub(super) fn exclusive(path: &Path) -> Result<Held> {
    let file = open(path)?;
    file.lock()
        .map_err(|e| refused(format!("unavailable: {e}")))?;
    Ok(Held { _file: file })
}

#[cfg(test)]
#[path = "ownership_latch_tests.rs"]
mod tests;
