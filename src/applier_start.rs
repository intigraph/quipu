//! Which stores may become a replica. A replica must equal its source at its
//! cursor, so it starts with no facts of its own (or from a verified
//! bootstrap, which refuses a store holding facts on its own).

use super::Applier;
use crate::error::Result;
use crate::store::Store;

impl Applier {
    /// Why `store` cannot become a new replica, or `None` when it can: it
    /// already follows a source, or it holds no facts (shapes and settings are
    /// not facts). Local facts with no cursor would be mixed into the replica,
    /// which would then equal no source while claiming to.
    ///
    /// # Errors
    /// [`Error::Sqlite`] if the store cannot be read.
    pub fn new_replica_refusal(store: &Store) -> Result<Option<String>> {
        let follows: bool = store.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master \
             WHERE type = 'table' AND name = 'replica_progress')",
            [],
            |r| r.get(0),
        )?;
        if follows
            && store
                .conn
                .query_row("SELECT EXISTS (SELECT 1 FROM replica_progress)", [], |r| {
                    r.get::<_, bool>(0)
                })?
        {
            return Ok(None);
        }
        let holds_facts: bool =
            store
                .conn
                .query_row("SELECT EXISTS (SELECT 1 FROM facts)", [], |r| r.get(0))?;
        Ok(holds_facts.then(|| {
            "a new replica must start with no facts; this store holds local facts".to_string()
        }))
    }
}
