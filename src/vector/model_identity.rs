//! Nullable provenance: old/raw vectors remain UNKNOWN until explicitly rebuilt.
use super::f32_slice_to_bytes;
use crate::{Result, Store};
use rusqlite::{Connection, params};

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA main.table_info(vectors)")?;
    let columns = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if !columns.iter().any(|c| c == "model_identity") {
        conn.execute_batch("ALTER TABLE main.vectors ADD COLUMN model_identity TEXT")?;
    }
    Ok(())
}

impl Store {
    pub(super) fn write_identified_embedding(
        &self,
        entity_id: i64,
        text: &str,
        embedding: &[f32],
        valid_from: &str,
        identity: Option<&str>,
    ) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO vectors (entity_id,text,embedding,valid_from,valid_to,model_identity) VALUES (?1,?2,?3,?4,NULL,?5)", params![entity_id,text,f32_slice_to_bytes(embedding),valid_from,identity])?;
        Ok(())
    }

    pub(super) fn current_vector_model_matches(
        &self,
        entity_id: i64,
        text: &str,
        identity: &str,
    ) -> Result<bool> {
        // A newer matching row cannot bless an older, still-current vector.
        let (total, matching): (i64,i64) = self.conn.query_row("SELECT COUNT(*), COALESCE(SUM(CASE WHEN text=?2 AND model_identity=?3 THEN 1 ELSE 0 END),0) FROM vectors WHERE entity_id=?1 AND valid_to IS NULL", params![entity_id,text,identity], |r| Ok((r.get(0)?,r.get(1)?)))?;
        Ok(total > 0 && total == matching)
    }

    pub(super) fn sqlite_embedding_model_counts(
        &self,
        identity: Option<&str>,
    ) -> Result<(u64, u64)> {
        let (mismatch, unknown): (i64,i64) = self.conn.query_row("SELECT COALESCE(SUM(CASE WHEN ?1 IS NOT NULL AND model_identity IS NOT NULL AND model_identity != ?1 THEN 1 ELSE 0 END),0), COALESCE(SUM(CASE WHEN ?1 IS NULL OR model_identity IS NULL THEN 1 ELSE 0 END),0) FROM vectors WHERE valid_to IS NULL", [identity], |r| Ok((r.get(0)?,r.get(1)?)))?;
        Ok((
            u64::try_from(mismatch).map_err(|e| crate::Error::Store(e.to_string()))?,
            u64::try_from(unknown).map_err(|e| crate::Error::Store(e.to_string()))?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::Result;
    use crate::vector::{KnowledgeVectorStore, VectorMatch};
    struct Unsupported;
    impl KnowledgeVectorStore for Unsupported {
        fn embed_entity(&self, _: i64, _: &str, _: &[f32], _: &str) -> Result<()> {
            Ok(())
        }
        fn close_embedding(&self, _: i64, _: &str) -> Result<()> {
            Ok(())
        }
        fn current_embedding_text(&self, _: i64) -> Result<Option<String>> {
            Ok(Some("same".into()))
        }
        fn vector_search(&self, _: &[f32], _: usize, _: Option<&str>) -> Result<Vec<VectorMatch>> {
            Ok(vec![])
        }
        fn vector_count(&self) -> Result<usize> {
            Ok(1)
        }
    }
    #[test]
    fn unsupported_backend_cannot_claim_known_match_or_zero_mismatch() {
        assert!(
            !Unsupported
                .current_embedding_matches(1, "same", "8:model")
                .unwrap()
        );
        assert_eq!(
            Unsupported.embedding_model_counts(Some("8:model")).unwrap(),
            None
        );
        Unsupported
            .embed_entity_identified(1, "same", &[1.0; 8], "1", Some("8:model"))
            .unwrap();
        assert!(
            !Unsupported
                .current_embedding_matches(1, "same", "8:model")
                .unwrap()
        );
    }
}
