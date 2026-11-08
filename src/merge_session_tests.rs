//! Private store fixtures, with feature-scoped shape-validation contracts.
use super::*;
use crate::store::{Datum, Store};
use crate::types::{Op, Value as Stored};

const TD: &str = "2026-01-10T00:00:00Z";
const TT: &str = "2026-01-03T00:00:00Z";
fn key(p: &str) -> Slot {
    ("<urn:e>".into(), format!("urn:{p}"))
}
fn setup() -> (Store, Request) {
    let mut store = Store::open_in_memory().unwrap();
    let e = store.intern("urn:e").unwrap();
    let mut datums = Vec::new();
    for p in ["p", "q"] {
        datums.push(Datum {
            entity: e,
            attribute: store.intern(&format!("urn:{p}")).unwrap(),
            value: Stored::Str("ours".into()),
            valid_from: "2026-01-02T00:00:00Z".into(),
            valid_to: None,
            op: Op::Assert,
        });
    }
    store
        .transact(&datums, "2026-01-02T00:00:00Z", Some("fixture"), None)
        .unwrap();
    let request=Request {store_id:store.store_id().unwrap(),root_tx:store.latest_tx_id().unwrap(),path:Path::Share,
        input_id:"incoming".into(),base_id:"base".into(),
        base:"<urn:e> <urn:p> \"base\" .\n<urn:e> <urn:q> \"base\" .\n".into(),
        ours:render(&crate::share_merge::root_graph(&store).unwrap().0),
        theirs:"<urn:e> <urn:p> \"theirs\" .\n<urn:e> <urn:q> \"theirs\" .\n<urn:f> <urn:r> \"clean\" .\n".into(),
        shapes:"@prefix sh: <http://www.w3.org/ns/shacl#> . [] sh:path <urn:p>; sh:maxCount 1 . [] sh:path <urn:q>; sh:maxCount 1 .".into(),
        profile:Profile {mode:Mode::PartialSlot,..Profile::default()},parent:None,active_slots:None};
    (store, request)
}

#[cfg(not(feature = "shacl"))]
#[test]
fn shape_bound_partial_merge_refuses_without_validation_before_any_write() {
    let (mut store, request) = setup();
    let before = store.latest_tx_id().unwrap();
    let root = crate::share_merge::root_graph(&store).unwrap().0;
    let choices = [Choice {
        slot: key("p"),
        decision: Decision::Theirs,
    }];
    let prepared = crate::merge_adapters::prepare_store(&store, request, &choices, TT, TD).unwrap();
    let id = prepared.id.clone();
    let error = crate::merge_pending::execute(
        &mut store,
        "urn:fixture:ledger",
        prepared,
        TT,
        TD,
        Some("fixture"),
    )
    .expect_err("shape-bound application requires validation support");
    assert!(
        error
            .to_string()
            .contains("shape validation feature required")
    );
    assert_eq!(before, store.latest_tx_id().unwrap());
    assert_eq!(root, crate::share_merge::root_graph(&store).unwrap().0);
    assert!(
        crate::merge_pending::load(&store, "urn:fixture:ledger", &id)
            .unwrap()
            .is_none()
    );
}

#[cfg(feature = "shacl")]
#[test]
fn p1_pending_resume_is_sequenced_idempotent_and_only_applies_the_remainder() {
    let (mut store, request) = setup();
    let choices = [Choice {
        slot: key("p"),
        decision: Decision::Theirs,
    }];
    let plan = crate::merge_adapters::prepare_store(&store, request, &choices, TT, TD).unwrap();
    let again = plan.clone();
    let first = crate::merge_pending::execute(
        &mut store,
        "urn:fixture:ledger",
        plan,
        TT,
        TD,
        Some("fixture"),
    )
    .unwrap();
    assert_eq!((first.applied, first.held), (2, 1));
    assert!(first.receive_tx.is_some());
    let head = store.latest_tx_id().unwrap();
    assert_eq!(
        crate::merge_pending::execute(
            &mut store,
            "urn:fixture:ledger",
            again,
            TT,
            "2026-01-11T00:00:00Z",
            Some("fixture")
        )
        .unwrap()
        .id,
        first.id
    );
    assert_eq!(head, store.latest_tx_id().unwrap());
    let choices = [Choice {
        slot: key("q"),
        decision: Decision::Theirs,
    }];
    let remainder = crate::merge_adapters::resume(
        &store,
        "urn:fixture:ledger",
        &first.id,
        &choices,
        "2026-01-11T00:00:00Z",
    )
    .unwrap();
    assert_eq!(remainder.request.parent.as_ref(), Some(&first.id));
    assert_eq!(remainder.plan.slots.len(), 1);
    let second = crate::merge_adapters::apply_store(
        &mut store,
        "urn:fixture:ledger",
        remainder,
        TT,
        "2026-01-11T00:00:00Z",
        Some("fixture"),
    )
    .unwrap();
    assert_eq!((second.applied, second.held), (1, 0));
    assert_ne!(first.id, second.id);
    assert!(
        crate::merge_adapters::resume(&store, "urn:fixture:ledger", &first.id, &choices, TD)
            .is_err()
    );
}

#[test]
fn uncommitted_prepared_clock_drift_refuses_before_any_sequence_or_receipt() {
    for (authored, decision) in [("2026-01-04T00:00:00Z", TD), (TT, "2026-01-11T00:00:00Z")] {
        let (mut store, request) = setup();
        let before = store.latest_tx_id().unwrap();
        let root = crate::share_merge::root_graph(&store).unwrap().0;
        let choices = [Choice {
            slot: key("p"),
            decision: Decision::Theirs,
        }];
        let prepared =
            crate::merge_adapters::prepare_store(&store, request, &choices, TT, TD).unwrap();
        assert!(
            !prepared.receive.is_empty(),
            "fixture must attempt real writes"
        );
        let id = prepared.id.clone();
        let error = crate::merge_pending::execute(
            &mut store,
            "urn:fixture:ledger",
            prepared,
            authored,
            decision,
            Some("fixture"),
        )
        .expect_err("clock substitution must refuse");
        assert!(
            error
                .to_string()
                .contains("prepared operation clocks differ")
        );
        assert_eq!(before, store.latest_tx_id().unwrap());
        assert_eq!(root, crate::share_merge::root_graph(&store).unwrap().0);
        assert!(
            crate::merge_pending::load(&store, "urn:fixture:ledger", &id)
                .unwrap()
                .is_none()
        );
    }
}

#[cfg(feature = "shacl")]
#[test]
fn atomic_held_and_late_oversized_record_both_leave_no_transactions() {
    let (mut store, mut request) = setup();
    request.profile.mode = Mode::Atomic;
    let before = store.latest_tx_id().unwrap();
    let plan = crate::merge_adapters::prepare_store(&store, request.clone(), &[], TT, TD).unwrap();
    let held = crate::merge_pending::execute(&mut store, "urn:fixture:ledger", plan, TT, TD, None)
        .unwrap();
    assert!(held.held > 0);
    assert_eq!(before, store.latest_tx_id().unwrap());
    request.profile.mode = Mode::PartialSlot;
    request.input_id = "x".repeat(8 * 1024 * 1024);
    let original = crate::share_merge::root_graph(&store).unwrap().0;
    let choices = [Choice {
        slot: key("p"),
        decision: Decision::Theirs,
    }];
    let plan = crate::merge_adapters::prepare_store(&store, request, &choices, TT, TD).unwrap();
    assert!(
        crate::merge_pending::execute(&mut store, "urn:fixture:ledger", plan, TT, TD, None)
            .is_err()
    );
    assert_eq!(before, store.latest_tx_id().unwrap());
    assert_eq!(original, crate::share_merge::root_graph(&store).unwrap().0);
}

#[test]
fn explicit_units_hold_without_any_member_receipt() {
    let (store, mut request) = setup();
    request.profile.units = vec![BTreeSet::from([key("p"), key("q")])];
    let choices = [Choice {
        slot: key("p"),
        decision: Decision::Theirs,
    }];
    let plan = crate::merge_adapters::prepare_store(&store, request, &choices, TT, TD).unwrap();
    assert_eq!(plan.held_units, vec![BTreeSet::from([key("p"), key("q")])]);
    assert!(plan.receive.iter().chain(&plan.decide).all(|op| match op {
        FactOp::Assert { value, .. }
        | FactOp::Close { value, .. }
        | FactOp::Receipt { value, .. } => value.subject.to_string() != "<urn:e>",
    }));
}

#[cfg(feature = "shacl")]
#[test]
fn decide_only_convergence_has_no_synthetic_receive_transaction() {
    let (mut store, mut request) = setup();
    request.theirs = request.ours.clone();
    let choices = [Choice {
        slot: key("p"),
        decision: Decision::Values(BTreeSet::from([Value::Term("\"chosen\"".into())])),
    }];
    let plan = crate::merge_adapters::prepare_store(&store, request, &choices, TT, TD).unwrap();
    assert!(plan.receive.is_empty() && !plan.decide.is_empty());
    let before = store.latest_tx_id().unwrap();
    let receipt =
        crate::merge_pending::execute(&mut store, "urn:fixture:ledger", plan, TT, TD, None)
            .unwrap();
    assert_eq!(receipt.receive_tx, None);
    assert_eq!(receipt.decide_tx, Some(before + 1));
    assert_eq!(
        store.latest_tx_id().unwrap(),
        before + 2,
        "decide then durable record only"
    );
    let loaded = crate::merge_pending::load(&store, "urn:fixture:ledger", &receipt.id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.receive_tx, None);
    assert_eq!(loaded.decide_tx, receipt.decide_tx);
}

#[cfg(feature = "shacl")]
#[test]
fn held_only_partial_plan_records_pending_without_receiving_empty_data() {
    let (mut store, mut request) = setup();
    request.theirs = "<urn:e> <urn:p> \"theirs\" .\n<urn:e> <urn:q> \"theirs\" .\n".into();
    let plan = crate::merge_adapters::prepare_store(&store, request, &[], TT, TD).unwrap();
    assert!(plan.receive.is_empty() && plan.decide.is_empty());
    assert_eq!(plan.plan.held, 2);
    let before = store.latest_tx_id().unwrap();
    let root = crate::share_merge::root_graph(&store).unwrap().0;
    let receipt =
        crate::merge_pending::execute(&mut store, "urn:fixture:ledger", plan, TT, TD, None)
            .unwrap();
    assert_eq!((receipt.receive_tx, receipt.decide_tx), (None, None));
    assert_eq!(
        store.latest_tx_id().unwrap(),
        before + 1,
        "durable pending record only"
    );
    assert_eq!(root, crate::share_merge::root_graph(&store).unwrap().0);
    assert_eq!(receipt.guards.len(), 2);
    assert!(
        crate::merge_pending::load(&store, "urn:fixture:ledger", &receipt.id)
            .unwrap()
            .is_some()
    );
}

#[cfg(feature = "shacl")]
#[path = "merge_parent_tests.rs"]
mod parent;

#[cfg(feature = "shacl")]
#[path = "merge_fresh_tests.rs"]
mod fresh;
