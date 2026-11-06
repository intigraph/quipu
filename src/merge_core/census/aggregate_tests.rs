use super::*;
use std::sync::{Arc, mpsc};
use std::time::Duration;

fn binding(profile: Option<[u8; 32]>) -> ProducerBinding {
    ProducerBinding {
        source_revision: [1; 32],
        profile_revision: profile,
    }
}

fn report(predicate: i64) -> SlotReport {
    SlotReport {
        graph_id: 7,
        predicate_id: predicate,
        base_evidence: BaseEvidence::Declared,
        shape_bound: BoundEvidence::Known { max_count: Some(1) },
        classification: Some(Change::Ours),
        would_hold_slot: Some(false),
        unknown_reason: None,
    }
}

fn snapshot(sink: &AggregateSink) -> Snapshot {
    match sink.try_snapshot() {
        SnapshotRead::Ready(snapshot) => snapshot,
        _ => panic!("fixture snapshot unexpectedly unavailable"),
    }
}

#[test]
fn revision_evidence_and_unknown_observations_never_mix_or_leak_reasons() {
    let sink = AggregateSink::default();
    assert_eq!(
        sink.try_record(binding(None), &[report(11), report(11)]),
        RecordOutcome::Accepted
    );
    assert_eq!(
        sink.try_record(binding(Some([2; 32])), &[report(11)]),
        RecordOutcome::Accepted
    );
    let unknown = SlotReport {
        base_evidence: BaseEvidence::Missing,
        shape_bound: BoundEvidence::Unknown,
        classification: None,
        would_hold_slot: None,
        unknown_reason: Some("PRIVATE_REASON_MUST_NOT_ESCAPE"),
        ..report(11)
    };
    assert_eq!(
        sink.try_record(binding(None), &[unknown]),
        RecordOutcome::Accepted
    );
    let result = snapshot(&sink);
    assert_eq!(result.accepted_batches, 3);
    assert_eq!(result.accepted_slot_observations, 4);
    assert_eq!(result.buckets.len(), 3);
    assert_eq!(result.buckets.iter().map(|(_, n)| n).sum::<u64>(), 4);
    assert!(result.buckets.iter().any(|(key, count)| {
        key.binding.profile_revision.is_none() && key.classification == "ours" && *count == 2
    }));
    assert!(result.buckets.iter().any(|(key, _)| {
        key.classification == "unknown" && !key.bound_known && key.would_hold_slot.is_none()
    }));
    let wire = serde_json::to_string(&result).unwrap();
    assert!(!wire.contains("PRIVATE"));
    assert!(result.health_counters_are_independent);
}

#[test]
fn capacity_drops_the_entire_batch_without_even_incrementing_existing_buckets() {
    let sink = AggregateSink::default();
    let reports: Vec<_> = (0..MAX_BUCKETS as i64).map(report).collect();
    assert_eq!(
        sink.try_record(binding(None), &reports),
        RecordOutcome::Accepted
    );
    let before = snapshot(&sink);
    assert_eq!(
        sink.try_record(binding(None), &[report(0), report(MAX_BUCKETS as i64)]),
        RecordOutcome::DroppedCapacity
    );
    let after = snapshot(&sink);
    assert_eq!(after.buckets, before.buckets);
    assert_eq!(after.accepted_batches, before.accepted_batches);
    assert_eq!(
        after.accepted_slot_observations,
        before.accepted_slot_observations
    );
    assert_eq!(after.dropped_capacity_batches, 1);
    assert_eq!(
        sink.try_record(binding(None), &[report(0)]),
        RecordOutcome::Accepted
    );
    assert_eq!(snapshot(&sink).accepted_batches, 2);
}

#[test]
fn contention_never_waits_and_a_busy_snapshot_is_not_a_successful_empty_result() {
    let sink = Arc::new(AggregateSink::default());
    let guard = sink.state.lock().unwrap();
    let child_sink = Arc::clone(&sink);
    let (sender, receiver) = mpsc::channel();
    let child = std::thread::spawn(move || {
        let outcome = child_sink.try_record(binding(None), &[report(11)]);
        let snapshot_busy = matches!(child_sink.try_snapshot(), SnapshotRead::Busy);
        sender.send((outcome, snapshot_busy)).unwrap();
    });
    // Release before asserting: a blocking-lock mutant must fail, not hang tests.
    let result = receiver.recv_timeout(Duration::from_secs(1));
    drop(guard);
    child.join().unwrap();
    assert_eq!(result.unwrap(), (RecordOutcome::DroppedBusy, true));
    let result = snapshot(&sink);
    assert_eq!(result.dropped_busy_batches, 1);
    assert_eq!(result.accepted_batches, 0);
    assert!(result.buckets.is_empty());
}

#[test]
fn invalid_or_oversized_batches_do_not_publish_partial_observations() {
    let sink = AggregateSink::default();
    let oversized: Vec<_> = (0..=MAX_SLOTS as i64).map(report).collect();
    assert_eq!(
        sink.try_record(binding(None), &oversized),
        RecordOutcome::DroppedInput
    );
    let inconsistent = SlotReport {
        unknown_reason: Some("shape-bound-unknown"),
        ..report(12)
    };
    assert_eq!(
        sink.try_record(binding(None), &[report(11), inconsistent]),
        RecordOutcome::DroppedInput
    );
    let result = snapshot(&sink);
    assert_eq!(result.dropped_input_batches, 2);
    assert_eq!(result.accepted_slot_observations, 0);
    assert!(result.buckets.is_empty());
}

#[test]
fn poisoning_stays_explicit_and_saturating_counts_never_wrap_to_zero() {
    let sink = AggregateSink::default();
    assert_eq!(
        sink.try_record(binding(None), &[report(11)]),
        RecordOutcome::Accepted
    );
    {
        let mut state = sink.state.lock().unwrap();
        *state.buckets.values_mut().next().unwrap() = u64::MAX - 1;
        state.accepted_batches = u64::MAX;
    }
    assert_eq!(
        sink.try_record(binding(None), &[report(11), report(11)]),
        RecordOutcome::Accepted
    );
    let result = snapshot(&sink);
    assert_eq!(result.accepted_batches, u64::MAX);
    assert_eq!(result.buckets[0].1, u64::MAX);
    assert!(result.counts_saturated);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = sink.state.lock().unwrap();
        panic!("synthetic poisoned-sink control");
    }));
    assert_eq!(
        sink.try_record(binding(None), &[report(11)]),
        RecordOutcome::DroppedPoisoned
    );
    assert!(matches!(sink.try_snapshot(), SnapshotRead::Poisoned));
    assert_eq!(sink.poisoned.load(Ordering::Relaxed), 1);
}
