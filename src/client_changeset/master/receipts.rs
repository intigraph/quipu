//! Protected local durable receipt, in the SAME outer transaction as facts.
#[cfg(feature = "shacl")]
use crate::{Result, Store};
#[cfg(feature = "shacl")]
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

/// A typed native receipt, not an invented RDF class or unsigned client claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangesetReceipt {
    pub operation_id: String,
    pub fingerprint: String,
    pub store_id: String,
    pub log_epoch: String,
    pub signer: String,
    pub key_id: String,
    pub public_key: String,
    pub accepted_tx: i64,
    pub accepted_at: String,
    pub verdict: String,
    pub post_state_hash: String,
}

#[cfg(feature = "shacl")]
pub(super) fn lookup(store: &Store, operation: &str) -> Result<Option<ChangesetReceipt>> {
    let row: Option<String> = store
        .conn
        .query_row(
            "SELECT receipt FROM client_changeset_receipts WHERE operation_id=?1",
            [operation],
            |r| r.get(0),
        )
        .optional()?;
    row.map(|s| serde_json::from_str(&s).map_err(|e| crate::Error::Serialization(e.to_string())))
        .transpose()
}

#[cfg(feature = "shacl")]
pub(super) fn insert(store: &Store, receipt: &ChangesetReceipt) -> Result<()> {
    if store.conn.is_autocommit() {
        return Err(super::refuse("receipt requires reserved writer"));
    }
    store.conn.execute(
        "INSERT INTO client_changeset_receipts VALUES(?1,?2)",
        rusqlite::params![
            receipt.operation_id,
            serde_json::to_string(receipt)
                .map_err(|e| crate::Error::Serialization(e.to_string()))?
        ],
    )?;
    Ok(())
}
