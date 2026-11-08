use super::*;
use std::collections::BTreeSet;

const TIME: &str = "2026-01-01T00:00:00Z";

fn slot(base: &[&str], ours: &[&str], incoming: &[&str], card: Card) -> SlotInput<String> {
    let side = |values: &[&str]| {
        values
            .iter()
            .map(|s| (s.to_string(), TIME.to_string()))
            .collect()
    };
    SlotInput {
        base: base
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>(),
        ours: side(ours),
        theirs: side(incoming),
        theirs_closed: BTreeMap::new(),
        card,
    }
}

fn batch(
    input: &SlotInput<String>,
    base: BaseEvidence,
    known: bool,
) -> BTreeMap<SlotKey, ObservationInput<'_>> {
    BTreeMap::from([(
        SlotKey {
            graph: 7,
            entity: 87654321,
            predicate: 11,
        },
        ObservationInput {
            slot: input,
            base,
            shape_bound_known: known,
        },
    )])
}

#[test]
fn unknown_base_and_unknown_bound_are_not_classified_as_unbounded() {
    let input = slot(&[], &["a"], &["b"], Card::Many(None));
    for (base, known, reason) in [
        (BaseEvidence::Missing, true, "read-base-unknown"),
        (BaseEvidence::Declared, false, "shape-bound-unknown"),
    ] {
        let report = observe_batch(&batch(&input, base, known), TIME).unwrap();
        assert_eq!(report[0].classification, None);
        assert_eq!(report[0].would_hold_slot, None);
        assert_eq!(report[0].unknown_reason, Some(reason));
    }
    let report = observe_batch(&batch(&input, BaseEvidence::Declared, true), TIME).unwrap();
    assert_eq!(report[0].classification, Some(Change::Divergent));
    let wire = serde_json::to_value(&report[0]).unwrap();
    assert_eq!(wire["shape_bound"]["status"], "known");
    assert!(wire["shape_bound"]["max_count"].is_null());
}

#[test]
fn native_default_decisions_cover_the_seven_merge_classes() {
    for (base, ours, incoming, card, change, held) in [
        (
            vec!["a"],
            vec!["a"],
            vec!["a"],
            Card::ONE,
            Change::Unchanged,
            false,
        ),
        (
            vec!["a"],
            vec!["b"],
            vec!["a"],
            Card::ONE,
            Change::Ours,
            false,
        ),
        (
            vec!["a"],
            vec!["a"],
            vec!["b"],
            Card::ONE,
            Change::Theirs,
            false,
        ),
        (
            vec!["a"],
            vec!["b"],
            vec!["b"],
            Card::ONE,
            Change::Convergent,
            false,
        ),
        (
            vec![],
            vec!["a"],
            vec!["b"],
            Card::Many(None),
            Change::Divergent,
            false,
        ),
        (
            vec!["a"],
            vec!["b"],
            vec!["c"],
            Card::ONE,
            Change::MaxcountExceeded,
            true,
        ),
        (
            vec!["a"],
            vec![],
            vec!["b"],
            Card::ONE,
            Change::DeleteReplace,
            true,
        ),
    ] {
        let input = slot(&base, &ours, &incoming, card);
        let before = input.clone();
        let report =
            observe_batch(&batch(&input, BaseEvidence::RetainedSnapshot, true), TIME).unwrap();
        assert_eq!(report[0].classification, Some(change));
        assert_eq!(report[0].would_hold_slot, Some(held));
        assert_eq!(input.base, before.base);
        assert_eq!(input.ours, before.ours);
        assert_eq!(input.theirs, before.theirs);
        assert_eq!(input.theirs_closed, before.theirs_closed);
    }
}

#[test]
fn reports_never_serialize_terms_entity_ids_or_authored_timestamps() {
    let input = slot(&["PRIVATE_BASE"], &[], &["PRIVATE_INCOMING"], Card::ONE);
    let report = observe_batch(&batch(&input, BaseEvidence::Declared, true), TIME).unwrap();
    let wire = serde_json::to_string(&report).unwrap();
    assert!(!wire.contains("PRIVATE"));
    assert!(!wire.contains("87654321"));
    assert!(!wire.contains(TIME));
    assert!(wire.contains("delete-replace"));
    assert!(wire.contains("\"would_hold_slot\":true"));
}

#[test]
fn input_budgets_refuse_the_whole_batch_without_partial_observations() {
    let large = "x".repeat(MAX_BYTES + 1);
    let input = slot(&[&large], &[], &[], Card::ONE);
    assert!(observe_batch(&batch(&input, BaseEvidence::Missing, false), TIME).is_err());

    let input = slot(&[], &[], &[], Card::ONE);
    let many_slots = (0..=MAX_SLOTS)
        .map(|entity| {
            (
                SlotKey {
                    graph: 7,
                    entity: entity as i64,
                    predicate: 11,
                },
                ObservationInput {
                    slot: &input,
                    base: BaseEvidence::Declared,
                    shape_bound_known: true,
                },
            )
        })
        .collect();
    assert!(observe_batch(&many_slots, TIME).is_err());

    let mut many_values = input.clone();
    many_values.base = (0..=MAX_VALUES).map(|i| i.to_string()).collect();
    assert!(observe_batch(&batch(&many_values, BaseEvidence::Declared, true), TIME).is_err());
}
