//! Process-lifetime admission for cooperating native Store writers.
//!
//! Shared leases preserve ordinary multi-handle use. A graph Owner upgrades
//! its supplied handle to exclusive admission before it becomes usable. This
//! is not persistent ownership, retirement fencing or protection from raw SQLite.
//!
//! The lease is an flock on a SIBLING file (`<db>.writer-lease`), never on the
//! database file itself. SQLite holds POSIX fcntl locks on the database, and
//! closing ANY descriptor of that file in a process drops every fcntl lock the
//! process holds on it. A lease descriptor on the database would therefore
//! silently release another connection's write lock when a lease is dropped,
//! e.g. after a failed exclusive upgrade. The sibling follows the same pattern
//! as `ownership_latch`'s `.lock` file.

use crate::{Error, Result};
use rusqlite::Connection;
use std::{fs::File, path::Path, sync::Arc};

pub(super) struct Lease {
    pub(super) file: File,
}

fn denied(message: impl std::fmt::Display) -> Error {
    Error::PolicyDenied(format!("native writer lease: {message}"))
}

impl Lease {
    pub(super) fn claim(conn: &Connection) -> Result<Option<Arc<Self>>> {
        let Some(path) = conn.path().filter(|p| !p.is_empty()) else {
            return Ok(None);
        };
        let sibling = super::ownership_latch::sibling(Path::new(path), ".writer-lease")?;
        let file = super::ownership_latch::no_follow(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false),
        )
        .open(&sibling)
        .map_err(denied)?;
        super::ownership_latch::regular(&file)?;
        rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockShared)
            .map_err(|_| denied("graph owner is active or admission unavailable"))?;
        Ok(Some(Arc::new(Self { file })))
    }

    // A failed upgrade may release this handle's shared lease on some Unix
    // kernels. Owner::prepare consumes the Store and drops it on failure;
    // the caller must never recover and use that unadmitted connection.
    pub(super) fn claim_exclusive(&self) -> Result<()> {
        rustix::fs::flock(
            &self.file,
            rustix::fs::FlockOperation::NonBlockingLockExclusive,
        )
        .map_err(|_| {
            Error::PolicyDenied("graph owner: another writable Store holds native admission".into())
        })
    }
}

#[cfg(test)]
#[path = "writer_lease_tests.rs"]
mod tests;
