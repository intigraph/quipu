//! Explicit bounded preparation of missing vectors for existing ROOT subjects.
use crate::{Error, Result, Store, vector::KnowledgeVectorStore};

/// Cursor and outcome for one bounded pass; callers explicitly pace passes.
#[derive(Debug, serde::Serialize)]
pub struct Progress {
    /// Resume after this local entity ID on the same database.
    pub next_cursor: i64,
    /// No further ROOT subject was observed in this pass's scan.
    pub complete: bool,
    /// New vectors committed by this call.
    pub embedded: usize,
    /// Concurrent changes skipped; rerun from cursor zero to revisit them.
    pub stale_skipped: usize,
}

/// Fill missing embeddings without replacing existing vectors. Each call scans
/// at most `limit + 1` distinct subjects and infers in batches of at most 32.
pub fn batch(store: &Store, after: i64, limit: usize) -> Result<Progress> {
    if after < 0 || !(1..=256).contains(&limit) {
        return Err(Error::InvalidValue(
            "backfill requires after >= 0 and batch-size in 1..=256".into(),
        ));
    }
    if store.has_vector_delegate()
        || store.has_local_vector_backend()
        || !store.attachments().is_empty()
    {
        return Err(Error::InvalidValue(
            "model backfill currently requires an unattached SQLite store".into(),
        ));
    }
    let provider = store
        .embedding_provider()
        .ok_or_else(|| Error::InvalidValue(crate::NO_PROVIDER_HELP.into()))?;
    let mut statement = store
        .conn
        .prepare("SELECT DISTINCT e FROM facts WHERE g=0 AND e>?1 AND op=1 ORDER BY e LIMIT ?2")?;
    let mut ids: Vec<i64> = statement
        .query_map(
            rusqlite::params![after, i64::try_from(limit + 1).expect("bounded batch size")],
            |r| r.get(0),
        )?
        .collect::<std::result::Result<_, _>>()?;
    let complete = ids.len() <= limit;
    ids.truncate(limit);
    let next_cursor = ids.last().copied().unwrap_or(after);
    let mut progress = Progress {
        next_cursor,
        complete,
        embedded: 0,
        stale_skipped: 0,
    };
    for chunk in ids.chunks(32) {
        let mut snapshots = Vec::new();
        for &id in chunk {
            if store.current_embedding_text(id)?.is_some() {
                continue;
            }
            let text = crate::build_entity_text(store, id)?;
            if !text.is_empty() {
                snapshots.push((id, text));
            }
        }
        if snapshots.is_empty() {
            continue;
        }
        let texts: Vec<_> = snapshots.iter().map(|(_, text)| text.as_str()).collect();
        let vectors = provider.embed_batch(&texts)?;
        if vectors.len() != snapshots.len()
            || vectors.iter().any(|v| {
                v.len() != provider.dimension() || v.is_empty() || v.iter().any(|x| !x.is_finite())
            })
        {
            return Err(Error::InvalidValue(
                "embedding provider returned invalid backfill vectors".into(),
            ));
        }
        // Inference holds no writer transaction. Recheck text and missingness
        // after acquiring the writer so another process's newer work wins.
        store.conn.execute_batch("BEGIN IMMEDIATE")?;
        let apply: Result<()> = (|| {
            for ((id, text), vector) in snapshots.iter().zip(&vectors) {
                if store.current_embedding_text(*id)?.is_some()
                    || crate::build_entity_text(store, *id)? != *text
                {
                    progress.stale_skipped += 1;
                    continue;
                }
                store.embed_entity(*id, text, vector, &crate::time::now_iso())?;
                progress.embedded += 1;
            }
            store.conn.execute_batch("COMMIT")?;
            Ok(())
        })();
        if let Err(error) = apply {
            let _ = store.conn.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    Ok(progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Provider;
    impl crate::EmbeddingProvider for Provider {
        fn embed_text(&self, _: &str) -> Result<Vec<f32>> {
            Ok(vec![1.0, 0.0])
        }
        fn dimension(&self) -> usize {
            2
        }
    }
    #[test]
    fn bounded_cursor_resumes_and_existing_vectors_are_preserved() {
        let mut store = Store::open_in_memory().unwrap();
        crate::ingest_rdf(&mut store,
            br#"<urn:a> <http://www.w3.org/2000/01/rdf-schema#label> "Alpha" . <urn:b> <http://www.w3.org/2000/01/rdf-schema#label> "Beta" ."#.as_slice(),
            oxrdfio::RdfFormat::Turtle, None, "2026-01-01T00:00:00Z", None, None,
        ).unwrap();
        store.set_embedding_provider(std::sync::Arc::new(Provider));
        let first = batch(&store, 0, 1).unwrap();
        assert_eq!(first.embedded, 1);
        assert!(!first.complete);
        let next = batch(&store, first.next_cursor, 1).unwrap();
        assert_eq!(next.embedded, 1);
        assert!(next.complete);
        assert_eq!(batch(&store, 0, 256).unwrap().embedded, 0);
        assert_eq!(store.vector_count().unwrap(), 2);
        assert!(batch(&store, 0, 257).is_err());
    }
}
