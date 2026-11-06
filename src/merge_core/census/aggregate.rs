//! Bounded source-stage aggregation; deliberately not connected to a writer.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, TryLockError};

use serde::Serialize;

use super::{BaseEvidence, BoundEvidence, Change, MAX_SLOTS, SlotReport};

const MAX_BUCKETS: usize = 256;

/// Caller-supplied revision evidence, not authentication or profile validation.
/// Missing profile evidence stays distinct from every known profile revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct ProducerBinding {
    pub source_revision: [u8; 32],
    pub profile_revision: Option<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct Bucket {
    pub binding: ProducerBinding,
    pub graph_id: i64,
    pub predicate_id: i64,
    pub base: &'static str,
    pub bound_known: bool,
    pub max_count: Option<usize>,
    pub classification: &'static str,
    pub would_hold_slot: Option<bool>,
}

impl Bucket {
    fn from_report(binding: ProducerBinding, report: &SlotReport) -> Option<Self> {
        let classified = report.classification.is_some();
        if classified != report.would_hold_slot.is_some()
            || classified == report.unknown_reason.is_some()
            || (classified && report.base_evidence == BaseEvidence::Missing)
            || (classified && matches!(report.shape_bound, BoundEvidence::Unknown))
        {
            return None;
        }
        let base = match report.base_evidence {
            BaseEvidence::Missing => "missing",
            BaseEvidence::Declared => "declared",
            BaseEvidence::RetainedSnapshot => "retained-snapshot",
        };
        let (bound_known, max_count) = match report.shape_bound {
            BoundEvidence::Unknown => (false, None),
            BoundEvidence::Known { max_count } => (true, max_count),
        };
        let classification = match report.classification {
            None => "unknown",
            Some(Change::Unchanged) => "unchanged",
            Some(Change::Ours) => "ours",
            Some(Change::Theirs) => "theirs",
            Some(Change::Convergent) => "convergent",
            Some(Change::Divergent) => "divergent",
            Some(Change::MaxcountExceeded) => "maxcount-exceeded",
            Some(Change::DeleteReplace) => "delete-replace",
        };
        Some(Self {
            binding,
            graph_id: report.graph_id,
            predicate_id: report.predicate_id,
            base,
            bound_known,
            max_count,
            classification,
            would_hold_slot: report.would_hold_slot,
        })
    }
}

#[derive(Default)]
struct State {
    buckets: BTreeMap<Bucket, u64>,
    accepted_batches: u64,
    accepted_slot_observations: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RecordOutcome {
    Accepted,
    DroppedBusy,
    DroppedPoisoned,
    DroppedInput,
    DroppedCapacity,
}

#[derive(Debug, Serialize)]
pub(crate) struct Snapshot {
    pub accepted_batches: u64,
    pub accepted_slot_observations: u64,
    pub dropped_busy_batches: u64,
    pub dropped_poisoned_batches: u64,
    pub dropped_input_batches: u64,
    pub dropped_capacity_batches: u64,
    pub counts_saturated: bool,
    /// Health counters are independently sampled, not an atomic delivery ledger.
    pub health_counters_are_independent: bool,
    pub buckets: Vec<(Bucket, u64)>,
}

pub(crate) enum SnapshotRead {
    Ready(Snapshot),
    Busy,
    Poisoned,
}

#[derive(Default)]
pub(crate) struct AggregateSink {
    state: Mutex<State>,
    busy: AtomicU64,
    poisoned: AtomicU64,
    input: AtomicU64,
    capacity: AtomicU64,
}

fn increment(counter: &AtomicU64) {
    // Lock-free saturation: a wrapped zero must never look like no drops.
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_add(1))
    });
}

impl AggregateSink {
    /// Never waits for the mutex. A dropped batch leaves every bucket unchanged.
    /// This is in-memory observation only, never a durable receipt or writer gate.
    pub(crate) fn try_record(
        &self,
        binding: ProducerBinding,
        reports: &[SlotReport],
    ) -> RecordOutcome {
        if reports.len() > MAX_SLOTS {
            increment(&self.input);
            return RecordOutcome::DroppedInput;
        }
        let mut state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => {
                increment(&self.busy);
                return RecordOutcome::DroppedBusy;
            }
            Err(TryLockError::Poisoned(_)) => {
                increment(&self.poisoned);
                return RecordOutcome::DroppedPoisoned;
            }
        };
        let Some(keys) = reports
            .iter()
            .map(|report| Bucket::from_report(binding, report))
            .collect::<Option<Vec<_>>>()
        else {
            increment(&self.input);
            return RecordOutcome::DroppedInput;
        };
        let new_keys: BTreeSet<_> = keys
            .iter()
            .filter(|key| !state.buckets.contains_key(*key))
            .collect();
        if new_keys.len() > MAX_BUCKETS.saturating_sub(state.buckets.len()) {
            increment(&self.capacity);
            return RecordOutcome::DroppedCapacity;
        }
        drop(new_keys);
        for key in keys {
            let count = state.buckets.entry(key).or_default();
            *count = count.saturating_add(1);
        }
        state.accepted_batches = state.accepted_batches.saturating_add(1);
        state.accepted_slot_observations = state
            .accepted_slot_observations
            .saturating_add(reports.len() as u64);
        RecordOutcome::Accepted
    }

    /// A busy/poisoned read is explicit, never an empty successful snapshot.
    pub(crate) fn try_snapshot(&self) -> SnapshotRead {
        let state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return SnapshotRead::Busy,
            Err(TryLockError::Poisoned(_)) => return SnapshotRead::Poisoned,
        };
        let health = [&self.busy, &self.poisoned, &self.input, &self.capacity]
            .map(|counter| counter.load(Ordering::Relaxed));
        let counts_saturated = health.contains(&u64::MAX)
            || state.accepted_batches == u64::MAX
            || state.accepted_slot_observations == u64::MAX
            || state.buckets.values().any(|value| *value == u64::MAX);
        SnapshotRead::Ready(Snapshot {
            accepted_batches: state.accepted_batches,
            accepted_slot_observations: state.accepted_slot_observations,
            dropped_busy_batches: health[0],
            dropped_poisoned_batches: health[1],
            dropped_input_batches: health[2],
            dropped_capacity_batches: health[3],
            counts_saturated,
            health_counters_are_independent: true,
            buckets: state.buckets.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        })
    }
}

#[cfg(test)]
#[path = "aggregate_tests.rs"]
mod tests;
