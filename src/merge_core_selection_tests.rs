//! Unrun source controls for P1 and transitive unit closure (no persistence).
use super::*;
use crate::merge_core::Card;

fn slot(conflict: bool) -> SlotInput<&'static str> {
    SlotInput {
        base: BTreeSet::from(["base"]),
        ours: BTreeMap::from([(if conflict { "ours" } else { "base" }, "2026-01-01".into())]),
        theirs: BTreeMap::from([("theirs", "2026-01-02".into())]),
        theirs_closed: BTreeMap::new(),
        card: Card::ONE,
    }
}

#[test]
fn p1_modes_hold_exactly_the_required_slots_and_no_receipts_for_held() {
    let inputs = BTreeMap::from([(1, slot(true)), (2, slot(true)), (3, slot(false))]);
    let decisions = BTreeMap::from([(1, Decision::Theirs)]);
    let entities = BTreeMap::from([(1, "e"), (2, "e"), (3, "f")]);
    for (mode, applied, held) in [
        (Mode::Atomic, 0, 1),
        (Mode::PartialEntity, 1, 2),
        (Mode::PartialSlot, 2, 1),
    ] {
        let plan = select(&inputs, &decisions, &entities, &[], mode, "2026-01-10").unwrap();
        assert_eq!((plan.applied, plan.held), (applied, held));
        assert!(
            plan.receive
                .iter()
                .chain(&plan.decide)
                .all(|(key, _)| !plan.slots[key].held)
        );
        if mode != Mode::Atomic {
            assert!(plan.slots[&2].receive.is_empty());
            assert!(plan.slots[&2].decide.is_empty());
        }
    }
}

#[test]
fn unit_and_entity_closure_is_transitive_and_keeps_unrelated_work() {
    let inputs = BTreeMap::from([
        (1, slot(true)),
        (2, slot(false)),
        (3, slot(false)),
        (4, slot(false)),
        (5, slot(false)),
    ]);
    let entities = BTreeMap::from([(1, "a"), (2, "b"), (3, "b"), (4, "c"), (5, "d")]);
    let units = [BTreeSet::from([1, 2]), BTreeSet::from([3, 4])];
    let plan = select(
        &inputs,
        &BTreeMap::new(),
        &entities,
        &units,
        Mode::PartialEntity,
        "2026-01-10",
    )
    .unwrap();
    assert_eq!((plan.applied, plan.held), (1, 4));
    assert!(plan.receive.iter().all(|(key, _)| *key == 5));
    assert!(plan.decide.is_empty());
}

#[test]
fn clean_units_apply_and_unknown_or_empty_units_are_refused() {
    let inputs = BTreeMap::from([(1, slot(false)), (2, slot(false))]);
    let entities = BTreeMap::from([(1, "e"), (2, "e")]);
    let plan = select(
        &inputs,
        &BTreeMap::new(),
        &entities,
        &[BTreeSet::from([1, 2])],
        Mode::PartialSlot,
        "2026-01-10",
    )
    .unwrap();
    assert_eq!((plan.applied, plan.held), (2, 0));
    for unit in [BTreeSet::new(), BTreeSet::from([9])] {
        assert!(
            select(
                &inputs,
                &BTreeMap::new(),
                &entities,
                &[unit],
                Mode::PartialSlot,
                "2026-01-10"
            )
            .is_err()
        );
    }
    assert!(
        select(
            &inputs,
            &BTreeMap::new(),
            &BTreeMap::<i32, &str>::new(),
            &[],
            Mode::PartialSlot,
            "2026-01-10"
        )
        .is_err()
    );
}
