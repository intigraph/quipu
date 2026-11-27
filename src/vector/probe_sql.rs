//! Survivor-metadata probes for vector search (the `sqlite_metadata` phase).

/// Survivor metadata for a current search; served by `idx_vectors_current`.
pub(crate) const CURRENT_TEXT_SQL: &str = "SELECT text, valid_from, valid_to FROM vectors \
     WHERE entity_id = ?1 AND valid_to IS NULL LIMIT 1";
/// Survivor metadata as of ?2. Descending: the newest version starting at or
/// before ?2 is the one valid at ?2, so the probe stops at its first row instead
/// of walking the entity's history from the oldest version.
pub(crate) const AS_OF_TEXT_SQL: &str = "SELECT text, valid_from, valid_to FROM vectors \
     WHERE entity_id = ?1 AND valid_from <= ?2 AND (valid_to IS NULL OR valid_to > ?2) \
     ORDER BY valid_from DESC LIMIT 1";
