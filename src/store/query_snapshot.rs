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

    /// Pin every mounted file before evaluating a composed read. The returned
    /// heads are per-source observations, not a globally atomic watermark.
    /// A caller sharing answers must independently fence every source.
    pub fn query_composed_snapshot<T>(
        &self,
        evaluate: impl FnOnce(i64, &[(String, i64)]) -> T,
    ) -> Result<T> {
        self.query_snapshot(|head| {
            let heads = self
                .attachments()
                .iter()
                .map(|source| {
                    super::attach::validate_alias(&source.alias)?;
                    let head = self.conn.query_row(
                        &format!(
                            "SELECT COALESCE(MAX(id),0) FROM {}.transactions",
                            source.alias
                        ),
                        [],
                        |row| row.get::<_, i64>(0),
                    )?;
                    Ok((source.alias.clone(), head))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(evaluate(head, &heads))
        })?
    }
}
