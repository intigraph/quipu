//! Values carried between staging and commit; no persistence or side effects.

use super::Datum;

/// Result of staging a transaction's rows into the open `quipu_transact`
/// savepoint, threaded to [`super::Store::after_commit_hooks`] after RELEASE.
pub(super) struct Staged {
    pub(super) tx_id: i64,
    /// What this write did, for `quipu_write_facts_total` (aegis-gwkd76). Taken
    /// from values staging computes anyway: no extra store read.
    pub(super) counts: StagedCounts,
    /// The COMPLETE set of changes this write made to the current-fact view, or
    /// `None` when the write path cannot vouch that it is complete.
    ///
    /// `Some` lets the resident read model be maintained instead of rebuilt.
    /// It is `None` whenever something closed or added facts that are not in
    /// this list — OWL domain/range inference extends the batch, and
    /// functional-property supersede closes prior values with a bulk
    /// `UPDATE ... WHERE v != ?` that never enumerates what it hit. Neither is
    /// reconstructible from the caller's datums, so the honest answer is to say
    /// so and let the model rebuild.
    pub(super) effective: Option<Vec<Datum>>,
    /// Datums actually asserted (skipping idempotent no-ops), for reactive
    /// observer notification.
    #[cfg(feature = "reactive-reasoner")]
    pub(super) asserts: Vec<Datum>,
    /// Datums actually retracted, for reactive observer notification.
    #[cfg(feature = "reactive-reasoner")]
    pub(super) retracts: Vec<Datum>,
}

/// Datum counts a staged write already knows (see `metrics::writes`).
#[derive(Clone, Copy)]
pub(super) struct StagedCounts {
    pub(super) inferred: usize,
    pub(super) asserted: usize,
    pub(super) retracted: usize,
    pub(super) superseded: usize,
}
