//! Process-lifetime admission for cooperating native Store writers.
//!
//! Shared leases preserve ordinary multi-handle use. A graph Owner upgrades
//! its supplied handle to exclusive admission before it becomes usable. This
//! is not persistent ownership, retirement fencing or protection from raw SQLite.

use crate::{Error, Result};
use rusqlite::Connection;
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    sync::Arc,
};

pub(super) struct Lease {
    pub(super) file: File,
}

impl Lease {
    pub(super) fn claim(conn: &Connection) -> Result<Option<Arc<Self>>> {
        let Some(path) = conn.path().filter(|p| !p.is_empty()) else {
            return Ok(None);
        };
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(path)
            .map_err(|e| Error::PolicyDenied(format!("native writer lease: {e}")))?;
        rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockShared).map_err(
            |_| {
                Error::PolicyDenied(
                    "native writer lease: graph owner is active or admission unavailable".into(),
                )
            },
        )?;
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
