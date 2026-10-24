//! Exact batched SQLite scan; the derived current-payload index is optional.
use rusqlite::{OptionalExtension, params};

use super::{TopK, VectorMatch, bytes_to_f32_slice, cosine_with_norms, squared_norm};
use crate::{error::Result, store::Store};

impl Store {
    /// One brute-force scan scoring every query; result `i` equals
    /// `scan_vectors(queries[i], limit, valid_at, None)`.
    ///
    /// Each query keeps a bounded top-`limit` instead of a score per row, and the
    /// survivors are ordered by score descending with scan order breaking ties,
    /// which is what the stable sort-then-truncate in `scan_vectors` yields.
    pub(super) fn scan_vectors_batch(
        &self,
        queries: &[&[f32]],
        limit: usize,
        valid_at: Option<&str>,
    ) -> Result<Vec<Vec<VectorMatch>>> {
        self.scan_vectors_batch_using(queries, limit, valid_at, true)
    }

    fn scan_vectors_batch_using(
        &self,
        queries: &[&[f32]],
        limit: usize,
        valid_at: Option<&str>,
        allow_cover: bool,
    ) -> Result<Vec<Vec<VectorMatch>>> {
        crate::time::check_request_deadline()?;
        if queries.is_empty() {
            return Ok(Vec::new());
        }
        let covered = allow_cover && valid_at.is_none()
            && queries.iter().all(|q| q.iter().all(|x| x.is_finite()))
            && self.conn.query_row(
                "SELECT 1 FROM main.sqlite_schema WHERE type = 'index' AND name = 'idx_vectors_current_payload_v1'",
                [], |row| row.get::<_, i64>(0),
            ).optional()?.is_some();
        let sql = if valid_at.is_some() {
            "SELECT entity_id, embedding FROM vectors \
             WHERE valid_from <= ?1 AND (valid_to IS NULL OR valid_to > ?1)"
        } else if covered {
            // Rowid is the legacy scan's tie key. Feeding that key into TopK
            // preserves finite-score results without sorting large blobs in SQL.
            "SELECT entity_id, embedding, rowid FROM vectors INDEXED BY idx_vectors_current_payload_v1 \
             WHERE valid_to IS NULL"
        } else {
            // Read-only pre-migration stores and unordered scores keep the
            // original row traversal, including legacy error behavior.
            "SELECT entity_id, embedding FROM vectors WHERE valid_to IS NULL ORDER BY rowid"
        };
        let scan_trace = crate::search_trace::phase(crate::search_trace::Phase::Scan);
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = if let Some(vt) = valid_at {
            stmt.query(params![vt])?
        } else {
            stmt.query([])?
        };

        let mut tops: Vec<TopK> = queries.iter().map(|_| TopK::new(limit)).collect();
        // Each query's squared norm once, not once per stored row.
        let query_norms: Vec<f64> = queries.iter().map(|q| squared_norm(q)).collect();
        let mut seq = 0usize;
        while let Some(row) = {
            let _timer = crate::private_7ek_profile::timer("vector_sql_next");
            rows.next()?
        } {
            // The scan is the long phase of a write's resolution pass; honour the
            // request deadline inside it, not only around it (aegis-xi0).
            if seq.is_multiple_of(64) {
                crate::time::check_request_deadline()?;
            }
            let _decode_timer = crate::private_7ek_profile::timer("vector_decode_norm");
            let entity_id: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            let stored = bytes_to_f32_slice(&blob);
            // The stored vector's squared norm once per row, not once per query.
            let stored_norm = squared_norm(&stored);
            let scan_sequence = if covered {
                let rowid: i64 = row.get(2)?;
                let Ok(rowid) = usize::try_from(rowid) else {
                    return self.scan_vectors_batch_using(queries, limit, valid_at, false);
                };
                // Unordered scores make the legacy comparator non-transitive;
                // malformed dimensions also have a first-error identity. Use
                // the original traversal for either instead of changing it.
                if !stored_norm.is_finite()
                    || stored.iter().any(|x| !x.is_finite())
                    || (!stored.is_empty() && queries.iter().any(|q| q.len() != stored.len()))
                {
                    return self.scan_vectors_batch_using(queries, limit, valid_at, false);
                }
                rowid
            } else {
                seq
            };
            drop(_decode_timer);
            let _score_timer = crate::private_7ek_profile::timer("vector_score_topk");
            for (query_index, ((query, top), query_norm)) in queries
                .iter()
                .zip(tops.iter_mut())
                .zip(query_norms.iter())
                .enumerate()
            {
                if query_index.is_multiple_of(16) {
                    crate::time::check_request_deadline()?;
                }
                if !stored.is_empty() && stored.len() != query.len() {
                    return Err(crate::Error::Store(format!(
                        "embedding dimension mismatch: query has {} dims but the stored \
                         vector for entity {} has {} — re-embed after changing the \
                         embedding model/dimension",
                        query.len(),
                        entity_id,
                        stored.len()
                    )));
                }
                top.offer(
                    cosine_with_norms(query, &stored, *query_norm, stored_norm),
                    scan_sequence,
                    entity_id,
                );
            }
            seq += 1;
        }
        crate::private_7ek_profile::add("vector_rows", seq);
        crate::private_7ek_profile::add("vector_queries", queries.len());
        crate::private_7ek_profile::add("vector_score_pairs", seq * queries.len());
        drop(scan_trace);

        let _metadata_trace = crate::search_trace::phase(crate::search_trace::Phase::Metadata);
        tops.into_iter()
            .map(|top| {
                crate::time::check_request_deadline()?;
                let matches = self.fetch_vector_matches(top.into_sorted(), valid_at)?;
                crate::time::check_request_deadline()?;
                Ok(matches)
            })
            .collect()
    }
}
