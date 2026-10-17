//! SQLite adapter for the vector backend contract.
use super::{KnowledgeVectorStore, VectorMatch};
use crate::{Result, Store};
use rusqlite::{OptionalExtension, params};

impl KnowledgeVectorStore for Store {
    fn text_search(
        &self,
        query: &str,
        limit: usize,
        valid_at: Option<&str>,
    ) -> Result<Vec<VectorMatch>> {
        if self.search_config().keyword {
            self.keyword_search(query, limit, valid_at, None)
        } else {
            Ok(vec![])
        }
    }

    fn ensure_fts_index(&self) -> Result<()> {
        if self.search_config().keyword {
            self.initialize_lexical_index()?;
        }
        Ok(())
    }
    fn embed_entity(
        &self,
        entity_id: i64,
        text: &str,
        embedding: &[f32],
        valid_from: &str,
    ) -> Result<()> {
        self.write_identified_embedding(entity_id, text, embedding, valid_from, None)
    }

    fn embed_entity_identified(
        &self,
        entity_id: i64,
        text: &str,
        embedding: &[f32],
        valid_from: &str,
        identity: Option<&str>,
    ) -> Result<()> {
        self.write_identified_embedding(entity_id, text, embedding, valid_from, identity)
    }

    fn current_embedding_matches(
        &self,
        entity_id: i64,
        text: &str,
        identity: &str,
    ) -> Result<bool> {
        self.current_vector_model_matches(entity_id, text, identity)
    }

    fn embedding_model_counts(&self, identity: Option<&str>) -> Result<Option<(u64, u64)>> {
        self.sqlite_embedding_model_counts(identity).map(Some)
    }

    fn close_embedding(&self, entity_id: i64, valid_to: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE vectors SET valid_to = ?1 WHERE entity_id = ?2 AND valid_to IS NULL",
            params![valid_to, entity_id],
        )?;
        Ok(())
    }

    fn current_embedding_text(&self, entity_id: i64) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT text FROM vectors WHERE entity_id = ?1 AND valid_to IS NULL \
             ORDER BY valid_from DESC LIMIT 1",
        )?;
        Ok(stmt
            .query_row(params![entity_id], |row| row.get(0))
            .optional()?)
    }

    fn vector_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        valid_at: Option<&str>,
    ) -> Result<Vec<VectorMatch>> {
        self.scan_vectors(query_embedding, limit, valid_at, None)
    }

    fn vector_search_where(
        &self,
        query: &[f32],
        limit: usize,
        valid_at: Option<&str>,
        keep: &mut dyn FnMut(i64) -> Result<bool>,
    ) -> Result<Vec<VectorMatch>> {
        self.scan_vectors(query, limit, valid_at, Some(keep))
    }

    fn vector_search_batch(
        &self,
        queries: &[&[f32]],
        limit: usize,
        valid_at: Option<&str>,
    ) -> Result<Vec<Vec<VectorMatch>>> {
        self.scan_vectors_batch(queries, limit, valid_at)
    }

    fn vector_count(&self) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM vectors WHERE valid_to IS NULL",
            [],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or(0))
    }
}
