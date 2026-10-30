//! Portable logical full-store wire values, shared by native and WASM appliers.
//! No file access, received SQL execution, credentials or producer activation.

use serde::{Deserialize, Serialize};

#[path = "store_replica/auth.rs"]
mod auth;
#[path = "store_replica/catalog.rs"]
pub(crate) mod catalog;
#[path = "store_replica/rows.rs"]
pub(crate) mod rows;
pub use auth::{FullStoreManifest, SnapshotTrust, manifest_hash, sign_revision, sign_snapshot};
pub(crate) use auth::{revision_hash, verify_revision};

/// A lossless SQLite cell. Integer values stay integers; blobs are hexadecimal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    /// SQLite storage class, never a SQL expression.
    pub kind: String,
    /// Null, integer, text, real encoded as text, or hexadecimal blob.
    pub value: serde_json::Value,
}

/// One locally owned column contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    /// Exact identifier from the local schema.
    pub name: String,
    /// Declared SQLite type.
    pub declared_type: String,
    /// Whether NULL is prohibited.
    pub not_null: bool,
    /// One-based order in the primary key, or zero.
    pub primary_key: i64,
}

/// One copied table. Received names never select a table outside this catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    /// Exact locally allowed table name.
    pub name: String,
    /// Exact ordered columns.
    pub columns: Vec<Column>,
    /// Locally inspected schema text, compared for compatibility, never executed
    /// from a received manifest.
    pub create_sql: String,
}

impl Table {
    pub(crate) fn key_indices(&self) -> Vec<usize> {
        let mut keys: Vec<_> = self
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.primary_key > 0)
            .collect();
        keys.sort_by_key(|(_, c)| c.primary_key);
        keys.into_iter().map(|(i, _)| i).collect()
    }
}

/// Identity of an explicitly initialized journal, not a credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalState {
    /// Changes whenever a journal is independently initialized.
    pub generation: String,
    /// Exact locally recognized table/column contract.
    pub catalog_digest: String,
}

/// One typed logical row mutation. It contains no executable SQL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    /// Contiguous source journal position.
    pub revision: i64,
    /// An exact table in the admitted catalog.
    pub table: String,
    /// `put` or `delete`.
    pub operation: String,
    /// Complete primary key, in declared order.
    pub key: Vec<Cell>,
    /// Complete ordered row for `put`; absent for `delete`.
    pub row: Option<Vec<Cell>>,
}

/// One committed source cut of the journal. Metadata-only changes have their
/// own revision even when the global fact transaction id did not advance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    /// Persistent source identity.
    pub source: crate::replica_manifest::Source,
    /// Journal identity and schema contract.
    pub journal: JournalState,
    /// Last previously published journal revision.
    pub since: i64,
    /// Complete committed prefix included in this batch.
    pub through: i64,
    /// Global source fact transaction at this same read snapshot.
    pub source_tx: i64,
    /// Every journal row in `(since, through]`, in order.
    pub changes: Vec<Change>,
}

/// Header of a logical snapshot. The whole catalog shares one source cut;
/// graph registration count does not split it into per-graph followers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotHeader {
    /// Persistent source lineage and epoch.
    pub source: crate::replica_manifest::Source,
    /// Exact initialized journal generation and schema policy.
    pub journal: JournalState,
    /// Tail starts strictly after this journal revision.
    pub revision: i64,
    /// Fact transaction at the same SQLite read snapshot.
    pub source_tx: i64,
    /// Locally recognized schemas, never executable peer SQL.
    pub catalog: Vec<Table>,
}

/// One full row, preserving original dictionary and transaction identifiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRow {
    /// Exact table in the header's admitted catalog.
    pub table: String,
    /// Every column in declared order, without a current-facts projection.
    pub cells: Vec<Cell>,
}

pub(crate) fn invalid(reason: impl Into<String>) -> crate::Error {
    crate::Error::InvalidValue(reason.into())
}

fn json_error(error: &serde_json::Error) -> crate::Error {
    invalid(format!("logical store JSON: {error}"))
}
