//! Frozen pre-change scan oracle and an explicitly invoked offline benchmark.
use crate::vector::*;
use std::time::Instant;

// Exact served 5d2fe613 scan, renamed only. Keep independent of the candidate.
impl Store {
    fn legacy_scan_vectors(
        &self,
        query_embedding: &[f32],
        limit: usize,
        valid_at: Option<&str>,
        keep: Option<&mut dyn FnMut(i64) -> Result<bool>>,
    ) -> Result<Vec<VectorMatch>> {
        // Score WITHOUT loading the `text` column. This is a
        // brute-force scan over EVERY vector; for a ~150k-entity store the text
        // of every entity dwarfs the vectors, and the old code materialized a
        // `VectorMatch { text, .. }` for ALL of them before truncating to top-K —
        // holding the whole store's text in RAM per search (the measured ~12GB
        // glibc-retained balloon). Keep only (entity_id, score) while scanning
        // (~16B/row); the embedding blob is transient per row; text + validity
        // are fetched below for just the `limit` survivors.
        let sql = if valid_at.is_some() {
            "SELECT entity_id, embedding FROM vectors \
             WHERE valid_from <= ?1 AND (valid_to IS NULL OR valid_to > ?1)"
        } else {
            "SELECT entity_id, embedding FROM vectors WHERE valid_to IS NULL"
        };

        let scan_trace = crate::search_trace::phase(crate::search_trace::Phase::Scan);
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = if let Some(vt) = valid_at {
            stmt.query(params![vt])?
        } else {
            stmt.query([])?
        };

        let mut scored: Vec<(i64, f64)> = Vec::new();
        while let Some(row) = rows.next()? {
            let entity_id: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;

            let stored = bytes_to_f32_slice(&blob);
            // Fail loud on a dimension mismatch instead of silently scoring 0.0
            // (hq-7v0): a stored vector whose dim ≠ the query/provider dim means
            // the embedding model or dimension changed without a re-embed, and
            // every cosine would be a meaningless 0.0.
            if !stored.is_empty() && stored.len() != query_embedding.len() {
                return Err(crate::Error::Store(format!(
                    "embedding dimension mismatch: query has {} dims but the stored \
                     vector for entity {} has {} — re-embed after changing the \
                     embedding model/dimension",
                    query_embedding.len(),
                    entity_id,
                    stored.len()
                )));
            }
            scored.push((entity_id, cosine_similarity(query_embedding, &stored)));
        }

        drop(scan_trace);
        let sort_trace = crate::search_trace::phase(crate::search_trace::Phase::Sort);
        // Sort by score descending, take top N.
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        // With a `keep` predicate, walk the WHOLE ranked list and keep the first
        // `limit` entities it admits. Filtering after a truncation starves a
        // small scope (aegis-rcz5ib.10): a graph of a few thousand entities
        // never survives a top-N cut taken over the whole store.
        let scored = match keep {
            None => {
                scored.truncate(limit);
                scored
            }
            Some(keep) => {
                let mut kept = Vec::with_capacity(limit);
                for (entity_id, score) in scored {
                    if kept.len() >= limit {
                        break;
                    }
                    if keep(entity_id)? {
                        kept.push((entity_id, score));
                    }
                }
                kept
            }
        };
        drop(sort_trace);
        let _metadata_trace = crate::search_trace::phase(crate::search_trace::Phase::Metadata);
        self.fetch_vector_matches(scored, valid_at)
    }
}

fn signature(rows: &[VectorMatch]) -> Vec<(i64, u64, &str)> {
    rows.iter()
        .map(|r| (r.entity_id, r.score.to_bits(), r.text.as_str()))
        .collect()
}

#[test]
#[ignore = "offline 20k-vector paired benchmark; invoke explicitly in release mode"]
fn paired_sqlite_vector_scan() {
    let store = Store::open_in_memory().unwrap();
    store.conn.execute_batch("BEGIN").unwrap();
    for id in 0..20_000_i64 {
        let embedding: Vec<f32> = (0..384)
            .map(|i| ((id * 17 + i * 3) as f32 * 0.01).sin())
            .collect();
        store.conn.execute(
            "INSERT INTO vectors(entity_id,text,embedding,valid_from,valid_to) VALUES(?1,?2,?3,'2026-01-01',NULL)",
            params![id, format!("fixture-{id} ").repeat(128), f32_slice_to_bytes(&embedding)],
        ).unwrap();
    }
    store.conn.execute_batch("COMMIT").unwrap();
    let query: Vec<f32> = (0..384).map(|i| (i as f32 * 0.07).cos()).collect();
    let old = store.legacy_scan_vectors(&query, 20, None, None).unwrap();
    let new = store.vector_search(&query, 20, None).unwrap();
    assert_eq!(signature(&old), signature(&new));
    // Exercise temporal selection and an admissibility predicate below top-K.
    for at in [None, Some("2026-02-01")] {
        let old = store
            .legacy_scan_vectors(&query, 10, at, Some(&mut |id| Ok(id % 127 == 0)))
            .unwrap();
        let new = store
            .vector_search_where(&query, 10, at, &mut |id| Ok(id % 127 == 0))
            .unwrap();
        assert_eq!(signature(&old), signature(&new));
    }
    let mut before = Vec::new();
    let mut after = Vec::new();
    for round in 0..8 {
        for candidate in if round % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let start = Instant::now();
            let rows = if candidate {
                store.vector_search(&query, 20, None)
            } else {
                store.legacy_scan_vectors(&query, 20, None, None)
            }
            .unwrap();
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(signature(&old), signature(&rows));
            if candidate {
                after.push(elapsed);
            } else {
                before.push(elapsed);
            }
        }
    }
    before.sort_by(f64::total_cmp);
    after.sort_by(f64::total_cmp);
    let median = |samples: &[f64]| (samples[3] + samples[4]) / 2.0;
    println!(
        "paired_sqlite_vector_scan rows=20000 dims=384 rounds=8 exact_scores_and_ranking=true before_ms={before:?} after_ms={after:?} median_before_ms={:.3} median_after_ms={:.3} ratio={:.4}",
        median(&before),
        median(&after),
        median(&after) / median(&before)
    );
}
