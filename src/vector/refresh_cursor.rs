//! Cursor read over entities that hold a CURRENT vector, for the bounded
//! `/embed_refresh` maintenance path (aegis-xc7).

use rusqlite::params;

use crate::error::Result;
use crate::store::Store;

impl Store {
    /// Up to `limit` distinct entity ids with a current (`valid_to IS NULL`)
    /// vector row, strictly after `after`, ascending.
    ///
    /// Walks the `(entity_id, valid_from)` primary key from the cursor, so a
    /// call costs its page, not the corpus: a resumable refresh never scans
    /// the whole vector table in one request (the aegis-tvlxr4 drain shape).
    pub fn current_vector_entities_after(&self, after: i64, limit: usize) -> Result<Vec<i64>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare_cached(
            "SELECT DISTINCT entity_id FROM vectors \
             WHERE entity_id > ?1 AND valid_to IS NULL \
             ORDER BY entity_id LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![after, limit], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<Vec<i64>, _>>()?)
    }
}
