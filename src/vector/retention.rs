//! Retention for superseded vectors (aegis-jio5.3).
//!
//! Every change to an entity's embedded text closes its current vector
//! (`valid_to` set) and inserts a new one, so without pruning the `vectors`
//! table keeps every version forever. Only `search --valid-at` reads closed
//! rows, so deleting the ones closed longer ago than the retention window
//! narrows point-in-time recall to that window and changes nothing else.

use rusqlite::params;

use crate::{Result, Store};

impl Store {
    /// Delete up to `limit` vectors that were superseded more than
    /// `retention_days` days before `now` (an ISO-8601 instant), and return
    /// how many were deleted.
    ///
    /// Only rows with `valid_to` set are candidates, so a current vector is
    /// never touched. `valid_to` is compared through `julianday`, which
    /// accepts both `Z` and numeric-offset forms; a value it cannot parse
    /// yields NULL and the row is kept rather than guessed at. A
    /// `retention_days` of 0 deletes nothing.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`] if the delete fails.
    pub fn prune_superseded_vectors(
        &self,
        retention_days: u32,
        now: &str,
        limit: usize,
    ) -> Result<usize> {
        if retention_days == 0 || limit == 0 {
            return Ok(0);
        }
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let deleted = self.conn.execute(
            "DELETE FROM vectors WHERE rowid IN (\
                 SELECT rowid FROM vectors \
                 WHERE valid_to IS NOT NULL \
                   AND julianday(valid_to) < julianday(?1) - ?2 \
                 LIMIT ?3)",
            params![now, f64::from(retention_days), limit],
        )?;
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;
    use crate::vector::KnowledgeVectorStore;

    const NOW: &str = "2026-11-02T00:00:00Z";

    fn store_with(rows: &[(i64, &str, Option<&str>)]) -> Store {
        let store = Store::open_in_memory().unwrap();
        for (entity, from, to) in rows {
            store.embed_entity(*entity, "t", &[0.5; 4], from).unwrap();
            if let Some(to) = to {
                store.close_embedding(*entity, to).unwrap();
            }
        }
        store
    }

    fn rows(store: &Store) -> Vec<(i64, String, Option<String>)> {
        let mut stmt = store
            .conn
            .prepare("SELECT entity_id, valid_from, valid_to FROM vectors ORDER BY 1, 2")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn deletes_only_versions_superseded_before_the_window() {
        let store = store_with(&[
            // Superseded 40 days before NOW: outside a 30-day window.
            (1, "2026-09-01T00:00:00Z", Some("2026-09-23T00:00:00Z")),
            // Superseded 10 days before NOW: inside the window, kept.
            (1, "2026-09-23T00:00:00Z", Some("2026-10-23T00:00:00Z")),
            // Current, though older than the window: never a candidate.
            (1, "2026-10-23T00:00:00Z", None),
            (2, "2026-01-01T00:00:00Z", None),
        ]);
        assert_eq!(store.prune_superseded_vectors(30, NOW, 1000).unwrap(), 1);
        let kept = rows(&store);
        assert_eq!(kept.len(), 3);
        assert!(
            kept.iter()
                .all(|(_, from, _)| from != "2026-09-01T00:00:00Z")
        );
        assert_eq!(kept.iter().filter(|(_, _, to)| to.is_none()).count(), 2);
    }

    #[test]
    fn an_offset_timestamp_is_compared_as_an_instant() {
        // 2026-10-02T23:00:00-05:00 is 2026-10-03T04:00:00Z: 29.8 days before
        // NOW, so a text comparison against a Z cutoff would wrongly delete it.
        let store = store_with(&[
            (1, "2026-09-01T00:00:00Z", Some("2026-10-02T23:00:00-05:00")),
            (1, "2026-10-02T23:00:00-05:00", None),
        ]);
        assert_eq!(store.prune_superseded_vectors(30, NOW, 1000).unwrap(), 0);
        assert_eq!(store.prune_superseded_vectors(29, NOW, 1000).unwrap(), 1);
    }

    #[test]
    fn an_unparseable_valid_to_is_kept() {
        let store = store_with(&[
            (1, "2020-01-01T00:00:00Z", Some("reactive")),
            (1, "reactive", None),
            (2, "2020-01-01T00:00:00Z", Some("1577923200")),
            (2, "2020-01-02T00:00:00Z", None),
        ]);
        assert_eq!(store.prune_superseded_vectors(30, NOW, 1000).unwrap(), 0);
        assert_eq!(rows(&store).len(), 4);
    }

    #[test]
    fn zero_days_keeps_everything() {
        let store = store_with(&[
            (1, "2020-01-01T00:00:00Z", Some("2020-01-02T00:00:00Z")),
            (1, "2020-01-02T00:00:00Z", None),
        ]);
        assert_eq!(store.prune_superseded_vectors(0, NOW, 1000).unwrap(), 0);
        assert_eq!(rows(&store).len(), 2);
    }

    #[test]
    fn the_limit_bounds_one_call() {
        let mut spec = Vec::new();
        let froms: Vec<String> = (0..5)
            .map(|d| format!("2020-01-0{}T00:00:00Z", d + 1))
            .collect();
        let tos: Vec<String> = (0..5)
            .map(|d| format!("2020-01-0{}T12:00:00Z", d + 1))
            .collect();
        for d in 0..5 {
            spec.push((1, froms[d].as_str(), Some(tos[d].as_str())));
        }
        spec.push((1, "2020-02-01T00:00:00Z", None));
        let store = store_with(&spec);
        assert_eq!(store.prune_superseded_vectors(30, NOW, 2).unwrap(), 2);
        assert_eq!(store.prune_superseded_vectors(30, NOW, 2).unwrap(), 2);
        assert_eq!(store.prune_superseded_vectors(30, NOW, 2).unwrap(), 1);
        assert_eq!(store.prune_superseded_vectors(30, NOW, 2).unwrap(), 0);
        assert_eq!(rows(&store).len(), 1);
    }

    #[test]
    fn retention_defaults_to_thirty_days() {
        let cfg = crate::config::EmbeddingConfig::default();
        assert_eq!(cfg.superseded_retention_days, 30);
        assert!(cfg.superseded_prune_max_per_pass > 0);
    }

    #[test]
    fn retention_reaches_the_config_from_toml() {
        let cfg: crate::config::EmbeddingConfig =
            toml::from_str("superseded_retention_days = 0\nsuperseded_prune_max_per_pass = 7")
                .unwrap();
        assert_eq!(cfg.superseded_retention_days, 0);
        assert_eq!(cfg.superseded_prune_max_per_pass, 7);
    }
}
