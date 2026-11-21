//! Caller-owned read snapshots for work that spans query serialization.
use super::Store;
use crate::Result;

impl Store {
    /// Evaluate with the main database pinned to the supplied transaction head.
    /// Existing caller-owned transactions retain their lifetime. This does not
    /// establish a cross-file watermark for attached databases.
    pub fn query_snapshot<T>(&self, evaluate: impl FnOnce(i64) -> T) -> Result<T> {
        let transaction = self
            .conn
            .is_autocommit()
            .then(|| self.conn.unchecked_transaction())
            .transpose()?;
        // The first main-file read pins the SQLite snapshot before evaluation.
        let head = self.latest_tx_id()?;
        let result = evaluate(head);
        if let Some(transaction) = transaction {
            transaction.commit()?;
        }
        Ok(result)
    }
}
