//! Logical core-table snapshot and mutation journal for a full-store follower.
//!
//! No filesystem database/WAL copy and no SQL received from a peer. The local
//! build owns the table/column contract. Journal installation is an explicit
//! writer operation, never a side effect of opening a store or serving a read.
//! Transport, signatures, shared-applier admission and read routing must still
//! qualify before this source primitive can serve a standby.

use crate::store_replica::catalog;
mod journal;
mod snapshot;

pub use catalog::{Cell, Column, Table};
pub use journal::{Change, JournalState, Revision, initialize, read_revision};
pub use snapshot::{SnapshotReceipt, write_snapshot};

fn invalid(reason: impl Into<String>) -> crate::Error {
    crate::Error::InvalidValue(reason.into())
}

fn json_error(error: &serde_json::Error) -> crate::Error {
    invalid(format!("logical store JSON: {error}"))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod bootstrap_tests;
