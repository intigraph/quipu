//! Observation-only timing of the SHACL write gate (aegis-wlf0).

use std::time::Duration;

use super::Store;

/// Where the post-state SHACL write gate spends its time (aegis-wlf0).
///
/// Summed over every gated transaction since the last take, so one request
/// that commits several graph batches reports their total.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PoststateProfile {
    /// Gated transactions that reached the shapes lookup.
    pub runs: u32,
    /// Reading and joining the loaded shape sets.
    pub load: Duration,
    /// Splitting the loaded shapes into reject and emit sets.
    pub split: Duration,
    /// Compiling or fetching the validator and its cardinality plan.
    pub plan: Duration,
    /// Gathering touched-focus context and validating it.
    pub check: Duration,
    /// Conditional (SET) admission: vocabulary and type context.
    pub conditional_prepare: Duration,
    /// Conditional admission: validation against the reject shapes.
    pub conditional_reject: Duration,
    /// Conditional admission: validation against the emit (warning) shapes.
    pub conditional_emit: Duration,
}

impl Store {
    /// The gate's accumulated time since the last call, then reset.
    pub fn take_poststate_profile(&mut self) -> PoststateProfile {
        std::mem::take(&mut self.poststate_profile)
    }

    /// Run `f` and return the gate's time spent inside it alone. The profile
    /// lives on the shared writer store, and `/knot` and `/episode` write
    /// through it without draining it, so whatever an earlier write left
    /// there is discarded first rather than attributed to `f`.
    pub fn observe_poststate<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> T,
    ) -> (T, PoststateProfile) {
        self.poststate_profile = PoststateProfile::default();
        let out = f(self);
        (out, self.take_poststate_profile())
    }
}
