//! Label scan and exact canonical-name matching.
use super::{EntityCandidate, id_list, is_slash_qualified_commit_id};
use crate::{error::Result, namespace, store::Store};

/// Every current `rdfs:label` in the composition, read ONCE.
///
/// The scan used to sit inside the per-entity path, so ingesting an episode of
/// N nodes ran N full scans of the label set, each decoding every value, and
/// running a Jaro-Winkler comparison and an id→IRI resolve per row: an O(N × L)
/// walk where O(N + L) was available. The rows do not change during a resolution
/// pass, so one scan serves the whole batch.
///
/// The IRI is resolved lazily — only for rows that actually clear the threshold
/// — because `store.resolve` is a query per id and most labels match nothing.
#[derive(Clone)]
pub(crate) struct LabelIndex {
    /// One entry per current `rdfs:label` fact, lowercased and profiled ONCE.
    ///
    /// `candidates` runs per node over every label. Comparing each with
    /// Jaro-Winkler was ~1 us per label, ~7 s for a 20-node episode against a
    /// 361k-label store (aegis-nmuwsi); [`LabelProfile`] lets it skip, exactly,
    /// the labels that cannot reach the threshold.
    pub(super) entries: Vec<std::sync::Arc<LabelEntry>>,
}

#[derive(Clone)]
pub(super) struct LabelEntry {
    pub(super) entity_id: i64,
    pub(super) lower: String,
    pub(super) profile: LabelProfile,
}

/// Slack added to [`LabelProfile::jaro_winkler_upper_bound`] so floating-point
/// rounding can only make pruning more conservative, never drop a candidate.
/// Far above any rounding error and far below any meaningful score gap.
pub(crate) const JW_BOUND_MARGIN: f64 = 1e-9;

/// Character count and a 64-bucket character histogram of a lowercased label.
///
/// Jaro's match count `m` is at most the multiset intersection of the two
/// strings' characters, and bucketing only merges counts, so the sum of
/// per-bucket minimums is still an upper bound on `m`. A bucket that would
/// overflow marks the profile unusable, and an unusable profile never prunes.
#[derive(Clone)]
pub(crate) struct LabelProfile {
    chars: usize,
    buckets: [u8; 64],
    exact: bool,
}

impl LabelProfile {
    pub(crate) fn of(lower: &str) -> Self {
        let mut buckets = [0u8; 64];
        let mut chars = 0;
        let mut exact = true;
        for c in lower.chars() {
            chars += 1;
            let b = &mut buckets[(u32::from(c) % 64) as usize];
            match b.checked_add(1) {
                Some(v) => *b = v,
                None => exact = false,
            }
        }
        Self {
            chars,
            buckets,
            exact,
        }
    }

    /// An upper bound on `strsim::jaro_winkler` between the two profiled strings.
    ///
    /// Jaro is `(m/a + m/b + (m - t)/m) / 3` with `m` bounded as above, so at most
    /// `(m/a + m/b + 1) / 3`; Winkler adds at most `4 * 0.1 * (1 - jaro)`, giving
    /// `0.6 * jaro + 0.4`. Returns 1.0 (never prunes) for an empty string or an
    /// unusable profile, where strsim's own edge cases apply.
    ///
    /// The result is padded by [`JW_BOUND_MARGIN`]: the bound and strsim reach the
    /// same value along different floating-point paths, so an exact bound can
    /// round BELOW a real score and prune a true candidate at the threshold.
    /// Measured: "aaaaaaaaaa" vs "aaaaaaabbb" bounds to 0.87999999999999989
    /// against a real 0.88 (dearing, review of #538).
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn jaro_winkler_upper_bound(&self, other: &Self) -> f64 {
        if self.chars == 0 || other.chars == 0 || !self.exact || !other.exact {
            return 1.0;
        }
        let common: usize = self
            .buckets
            .iter()
            .zip(other.buckets.iter())
            .map(|(a, b)| usize::from(*a.min(b)))
            .sum();
        let m = common.min(self.chars).min(other.chars) as f64;
        let jaro = (m / self.chars as f64 + m / other.chars as f64 + 1.0) / 3.0;
        0.6 * jaro + 0.4 + JW_BOUND_MARGIN
    }
}

impl LabelIndex {
    /// Scan the composed fact source for current `rdfs:label` values.
    ///
    /// # Errors
    /// Propagates store and decode failures.
    pub(crate) fn build(store: &Store) -> Result<std::sync::Arc<Self>> {
        super::cache::get(store)
    }

    pub(super) fn scan(store: &Store) -> Result<Self> {
        Self::build_filtered(store, None)
    }

    /// Only the labels asserted in transactions after `after_tx`: the delta a
    /// two-phase `/episode` re-checks under the writer (aegis-xi0).
    ///
    /// # Errors
    /// Propagates store and decode failures.
    pub(crate) fn build_since(store: &Store, after_tx: i64) -> Result<Self> {
        Self::build_filtered(store, Some(after_tx))
    }

    fn build_filtered(store: &Store, after_tx: Option<i64>) -> Result<Self> {
        // `lookup` is main-scoped, and a layer interns `rdfs:label` in ITS OWN
        // term space — so on a composed store the local id matches none of the
        // layer's label rows, and on a store whose main file has no labels at
        // all there is no local id to look up. Either way the union would be
        // read with a predicate that selects nothing. `lookup_all` is #76's
        // answer: every id the IRI denotes across the composition, matched with
        // `a IN (…)`, which is byte-identical to `a = ?` when there are none.
        let rdfs_label_iri = format!("{}label", namespace::RDFS);
        let label_ids = store.lookup_all(&rdfs_label_iri)?;
        if label_ids.is_empty() {
            return Ok(Self {
                entries: Vec::new(),
            }); // No labels interned yet.
        }

        // Schema: facts(e, a, v BLOB, tx, valid_from, valid_to, op).
        // `facts_source()` is the literal `facts` table with no attachments and
        // the `UNION ALL` over local + layers with them, so this one statement
        // reads the composition without knowing whether there is one.
        let since = after_tx.map_or_else(String::new, |tx| format!(" AND tx > {tx}"));
        let sql = format!(
            "SELECT e, v FROM {} WHERE a IN ({}) AND valid_to IS NULL AND op = 1{since}",
            store.facts_source(),
            id_list(&label_ids),
        );
        let mut stmt = store.conn.prepare(&sql)?;

        let rows = stmt.query_map([], |row| {
            let entity_id: i64 = row.get(0)?;
            let v_bytes: Vec<u8> = row.get(1)?;
            Ok((entity_id, v_bytes))
        })?;

        let mut entries = Vec::new();
        for row in rows {
            crate::time::check_request_deadline()?;
            let (entity_id, v_bytes) = row?;
            // Only string values are labels.
            let Ok(crate::types::Value::Str(label)) = crate::types::Value::from_bytes(&v_bytes)
            else {
                continue;
            };
            // An attached layer and the local store can intern the same IRI at
            // different ids; canonicalising here means one entity yields one
            // candidate rather than one per layer that mentions it.
            let lower = label.to_lowercase();
            entries.push(std::sync::Arc::new(LabelEntry {
                entity_id: store.canonical_id(entity_id)?,
                profile: LabelProfile::of(&lower),
                lower,
            }));
        }
        Ok(Self { entries })
    }

    /// The canonical entity id of every label in this index.
    pub(crate) fn entity_ids(&self) -> impl Iterator<Item = i64> + '_ {
        self.entries.iter().map(|entry| entry.entity_id)
    }

    /// Candidates for `name` by exact (case-insensitive) then Jaro-Winkler match.
    pub(crate) fn candidates(
        &self,
        store: &Store,
        name: &str,
        threshold: f64,
    ) -> Result<Vec<EntityCandidate>> {
        let name_lower = name.to_lowercase();
        let name_profile = LabelProfile::of(&name_lower);
        let name_is_commit = is_slash_qualified_commit_id(&name_lower);
        let mut candidates = Vec::new();
        for entry in &self.entries {
            crate::time::check_request_deadline()?;
            let (entity_id, label_lower) = (&entry.entity_id, &entry.lower);

            // Exact match (case-insensitive).
            if name_lower == *label_lower {
                candidates.push(EntityCandidate {
                    iri: store.resolve(*entity_id)?,
                    score: 1.0,
                    matched_on: "canonical_name:exact".to_string(),
                });
                continue;
            }

            // Canonical commit node names identify immutable objects. Two
            // different hashes are distinct even when their shared
            // `commit/<repo>/` prefix gives them a high Jaro-Winkler score.
            // Keep exact matching above for idempotent reuse, and keep this
            // deliberately narrower than a generic kind-prefix exemption.
            if name_is_commit && is_slash_qualified_commit_id(label_lower) {
                continue;
            }

            // A label that cannot reach the threshold is skipped without the
            // ~1 us Jaro-Winkler comparison; the bound never undercuts it.
            if name_profile.jaro_winkler_upper_bound(&entry.profile) < threshold {
                continue;
            }

            // Jaro-Winkler similarity.
            let jw = strsim::jaro_winkler(&name_lower, label_lower);
            if jw >= threshold {
                candidates.push(EntityCandidate {
                    iri: store.resolve(*entity_id)?,
                    score: jw,
                    matched_on: format!("canonical_name:jaro_winkler:{jw:.2}"),
                });
            }
        }
        Ok(candidates)
    }
}
