use super::*;
use crate::fluent::{Log, Observation, Order, Policy};

fn append(log: &mut Log, fluent: &str, tx: u64, n: i64) {
    log.append(Observation {
        fluent: fluent.into(),
        order: Order {
            tx,
            writer: "urn:writer".into(),
            event: format!("urn:event:{fluent}:{tx}"),
        },
        as_of: "2026-11-14T00:00:00Z".into(),
        recorded_on: "2026-11-14T00:00:00Z".into(),
        value: Scalar::Integer(n),
    })
    .unwrap();
}

#[test]
fn sum_transaction_emits_once_and_unrelated_scope_emits_nothing() {
    let mut log = Log::default();
    append(&mut log, "urn:count", 1, 2);
    let first = log.current("urn:count", Policy::Sum).unwrap().unwrap();
    let mut sub = Subscription::new("urn:graph", "urn:count", 1, Some(&first)).unwrap();
    append(&mut log, "urn:other", 2, 99);
    let other = log.current("urn:other", Policy::Sum).unwrap().unwrap();
    assert!(
        sub.advance("urn:graph", "urn:other", 2, &other)
            .unwrap()
            .is_none()
    );
    assert_eq!(sub.scanned(), 1);
    append(&mut log, "urn:count", 2, 3);
    let next = log.current("urn:count", Policy::Sum).unwrap().unwrap();
    let frame = sub
        .advance("urn:graph", "urn:count", 2, &next)
        .unwrap()
        .unwrap();
    assert_eq!(frame.value, BTreeSet::from([Scalar::Integer(5)]));
    assert_eq!(frame.tx, 2);
    assert!(
        sub.advance("urn:graph", "urn:count", 2, &next)
            .unwrap()
            .is_none()
    );
    assert!(
        sub.advance("urn:foreign", "urn:count", 3, &other)
            .unwrap()
            .is_none()
    );
    assert_eq!(sub.scanned(), 2);
}

#[test]
fn equal_value_new_provenance_advances_quietly_and_conflict_preserves_state() {
    let mut log = Log::default();
    append(&mut log, "urn:max", 1, 5);
    let first = log.current("urn:max", Policy::Max).unwrap().unwrap();
    let mut sub = Subscription::new("urn:g", "urn:max", 1, Some(&first)).unwrap();
    append(&mut log, "urn:max", 2, 5);
    let same = log.current("urn:max", Policy::Max).unwrap().unwrap();
    assert_ne!(first.produced_by, same.produced_by);
    assert!(sub.advance("urn:g", "urn:max", 2, &same).unwrap().is_none());
    assert_eq!(sub.scanned(), 2);
    let different = Current {
        values: BTreeSet::from([Scalar::Integer(6)]),
        produced_by: BTreeSet::new(),
    };
    assert!(sub.advance("urn:g", "urn:max", 2, &different).is_err());
    assert!(sub.advance("urn:g", "urn:max", 1, &different).is_err());
    assert_eq!(sub.scanned(), 2);
    assert_eq!(
        sub.advance("urn:g", "urn:max", 3, &different)
            .unwrap()
            .unwrap()
            .tx,
        3
    );
}

#[test]
fn rejected_projection_does_not_consume_cursor_or_replace_baseline() {
    let good = Current {
        values: BTreeSet::from([Scalar::Integer(1)]),
        produced_by: BTreeSet::new(),
    };
    let mut sub = Subscription::new("urn:g", "urn:f", 1, Some(&good)).unwrap();
    for values in [
        BTreeSet::new(),
        BTreeSet::from([Scalar::Term("x".repeat(MAX_FRAME_BYTES))]),
    ] {
        assert!(
            sub.advance(
                "urn:g",
                "urn:f",
                2,
                &Current {
                    values,
                    produced_by: BTreeSet::new()
                }
            )
            .is_err()
        );
        assert_eq!(sub.scanned(), 1);
    }
    assert!(sub.advance("urn:g", "urn:f", 2, &good).unwrap().is_none());
    assert!(Subscription::new("not an iri", "urn:f", 0, None).is_err());
    assert!(Subscription::new("urn:g", "urn:f", -1, None).is_err());
}
