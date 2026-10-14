//! Fail-closed transaction pins at the remote read boundary.

use serde_json::Value;

use crate::{Error, Result, Store};

/// Validate a requested file-local transaction without silently selecting now.
/// An absent field requests present state; JSON null is not an absent field.
pub fn requested_tx(store: &Store, input: &Value) -> Result<Option<i64>> {
    let Some(value) = input.get("tx") else {
        return Ok(None);
    };
    let tx = value.as_i64().filter(|tx| *tx > 0).ok_or_else(|| {
        Error::InvalidValue("tx must be a positive signed 64-bit JSON integer".into())
    })?;
    if input.get("valid_at").is_some() || input.get("fork").is_some() {
        return Err(Error::InvalidValue(
            "tx cannot be combined with valid_at or fork: mixed read clocks are unsupported".into(),
        ));
    }
    if store.has_attachments() {
        return Err(Error::InvalidValue(
            "tx is unsupported for attached stores: transaction IDs are file-local".into(),
        ));
    }
    if tx > store.latest_tx_id()? || store.get_transaction(tx)?.is_none() {
        return Err(Error::InvalidValue(format!(
            "tx {tx} does not identify an existing committed source transaction"
        )));
    }
    Ok(Some(tx))
}
