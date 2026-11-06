use super::*;
use crate::merge_core::{Card, SlotInput};

fn snapshot(sink: &AggregateSink) -> Snapshot {
    match sink.try_snapshot() {
        SnapshotRead::Ready(s) => s,
        _ => panic!("fixture unavailable"),
    }
}

fn prepare(sink: &AggregateSink) -> StagedObservation<'_> {
    let slot = SlotInput {
        base: BTreeSet::<String>::new(),
        ours: BTreeMap::new(),
        theirs: BTreeMap::new(),
        theirs_closed: BTreeMap::new(),
        card: Card::ONE,
    };
    let slots = BTreeMap::from([(
        SlotKey {
            graph: 7,
            entity: 123456789,
            predicate: 11,
        },
        ObservationInput {
            slot: &slot,
            base: BaseEvidence::Declared,
            shape_bound_known: true,
        },
    )]);
    match sink.try_stage(
        ShadowMode::Observe,
        ProducerBinding {
            source_revision: [1; 32],
            profile_revision: Some([2; 32]),
        },
        &slots,
        "2026-01-01T00:00:00Z",
    ) {
        StageOutcome::Prepared(stage) => stage,
        _ => panic!("fixture planning refused"),
    }
}

#[test]
fn captured_reports_outlive_request_and_wait_for_explicit_completion() {
    let sink = AggregateSink::default();
    let staged = prepare(&sink); // request/slot locals have already been dropped
    assert_eq!(snapshot(&sink).accepted_batches, 0);
    assert_eq!(
        staged.finish(WriteOutcome::Committed { transaction: 42 }),
        FinalizedObservation::DeclaredCommitted {
            transaction: 42,
            recording: RecordOutcome::Accepted
        }
    );
    let s = snapshot(&sink);
    assert_eq!(s.accepted_batches, 1);
    assert_eq!(s.staged_abandoned_batches, 0);
    assert!(!serde_json::to_string(&s).unwrap().contains("123456789"));
}

#[test]
fn rollback_unknown_and_abandonment_are_distinct_without_publication() {
    let sink = AggregateSink::default();
    assert_eq!(
        prepare(&sink).finish(WriteOutcome::RolledBack),
        FinalizedObservation::DiscardedRollback
    );
    assert_eq!(
        prepare(&sink).finish(WriteOutcome::Unknown),
        FinalizedObservation::DiscardedUnknown
    );
    assert_eq!(
        prepare(&sink).finish(WriteOutcome::Committed { transaction: 0 }),
        FinalizedObservation::DiscardedUnknown
    );
    assert_eq!(
        prepare(&sink).finish(WriteOutcome::Committed {
            transaction: u64::MAX
        }),
        FinalizedObservation::DiscardedUnknown
    );
    drop(prepare(&sink));
    let s = snapshot(&sink);
    assert_eq!(s.staged_batches, 5);
    assert_eq!(s.staged_rollback_batches, 1);
    assert_eq!(s.staged_unknown_batches, 3);
    assert_eq!(s.staged_abandoned_batches, 1);
    assert!(s.buckets.is_empty());
    assert_eq!(s.accepted_batches, 0);
}

#[test]
fn no_data_change_is_not_a_commit_and_contention_does_not_rewrite_outcome() {
    let sink = AggregateSink::default();
    assert_eq!(
        prepare(&sink).finish(WriteOutcome::NoDataChange),
        FinalizedObservation::NoDataChange {
            recording: RecordOutcome::Accepted
        }
    );
    let stage = prepare(&sink);
    let guard = sink.state.lock().unwrap();
    assert_eq!(
        stage.finish(WriteOutcome::Committed { transaction: 43 }),
        FinalizedObservation::DeclaredCommitted {
            transaction: 43,
            recording: RecordOutcome::DroppedBusy
        }
    );
    drop(guard);
    let s = snapshot(&sink);
    assert_eq!(s.accepted_batches, 1);
    assert_eq!(s.dropped_busy_batches, 1);
}

#[test]
fn default_disabled_and_planning_refusal_never_create_staged_state() {
    let sink = AggregateSink::default();
    let binding = ProducerBinding {
        source_revision: [1; 32],
        profile_revision: None,
    };
    assert!(matches!(
        sink.try_stage(ShadowMode::default(), binding, &BTreeMap::new(), "invalid"),
        StageOutcome::Disabled
    ));
    let td = "x".repeat(5 * 1024 * 1024);
    assert!(matches!(
        sink.try_stage(ShadowMode::Observe, binding, &BTreeMap::new(), &td),
        StageOutcome::PlanningRefused
    ));
    let s = snapshot(&sink);
    assert_eq!(s.staged_batches, 0);
    assert_eq!(s.disabled_batches, 1);
    assert_eq!(s.planning_refused_batches, 1);
}
