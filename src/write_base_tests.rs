use super::*;
use crate::resolver_contract::ContentDigest;

fn current() -> WriterSnapshot {
    WriterSnapshot {
        store_id: "store-a".into(),
        log_epoch: "epoch-a".into(),
        graph: "http://example.test/graph".into(),
        oldest_complete_tx: 4,
        head_tx: 10,
    }
}

fn declared() -> SourceBinding {
    let now = current();
    SourceBinding {
        store_id: now.store_id,
        log_epoch: now.log_epoch,
        graph: now.graph,
        base_tx: 6,
        base_slice_hash: ContentDigest([0; 32]),
        scope: Some(Vec::new()),
        inputs: Vec::new(),
    }
}

#[test]
fn declared_base_is_not_replaced_with_current_head() {
    let source = declared();
    let result =
        resolve_write_base(Some(&source), &current(), LegacyBasePolicy::RequireDeclared).unwrap();
    assert_eq!(result.base_tx, 6);
    assert!(!result.legacy_unbased);
    assert!(std::ptr::eq(result.source.unwrap(), &source));
    assert_eq!(result.source.unwrap().scope, Some(Vec::new()));
}

#[test]
fn compatibility_is_explicit_and_observable() {
    assert!(resolve_write_base(None, &current(), LegacyBasePolicy::RequireDeclared).is_err());
    let result = resolve_write_base(None, &current(), LegacyBasePolicy::UseCurrentAndFlag).unwrap();
    assert_eq!(result.base_tx, 10);
    assert!(result.legacy_unbased);
    assert!(result.source.is_none());
}

#[test]
fn unavailable_history_never_degrades_to_legacy_mode() {
    for tx in [0, 3, 11, u64::MAX] {
        let mut source = declared();
        source.base_tx = tx;
        assert!(
            resolve_write_base(
                Some(&source),
                &current(),
                LegacyBasePolicy::UseCurrentAndFlag
            )
            .is_err()
        );
    }
    for tx in [4, 10] {
        let mut source = declared();
        source.base_tx = tx;
        assert_eq!(
            resolve_write_base(Some(&source), &current(), LegacyBasePolicy::RequireDeclared)
                .unwrap()
                .base_tx,
            tx
        );
    }
}

#[test]
fn store_epoch_and_graph_are_not_interchangeable() {
    for field in 0..3 {
        let mut source = declared();
        match field {
            0 => source.store_id = "store-b".into(),
            1 => source.log_epoch = "epoch-b".into(),
            _ => source.graph = "http://example.test/other".into(),
        }
        assert!(
            resolve_write_base(
                Some(&source),
                &current(),
                LegacyBasePolicy::UseCurrentAndFlag
            )
            .is_err()
        );
    }
}

#[test]
fn malformed_capture_refuses_even_an_unbased_legacy_write() {
    for field in 0..4 {
        let mut now = current();
        match field {
            0 => now.store_id.clear(),
            1 => now.log_epoch.clear(),
            2 => now.graph.clear(),
            _ => now.oldest_complete_tx = 11,
        }
        assert!(resolve_write_base(None, &now, LegacyBasePolicy::UseCurrentAndFlag).is_err());
    }
}

fn slot(name: &str) -> crate::resolver_contract::Slot {
    crate::resolver_contract::Slot {
        subject: format!("<http://example.test/{name}>"),
        predicate: "<http://example.test/p>".into(),
    }
}

#[test]
fn scoped_preparation_requires_exact_slots_without_order_dependence() {
    let mut source = declared();
    source.scope = Some(vec![slot("a"), slot("b")]);
    let prepared = prepare_scoped_write_base(
        Some(&source),
        &current(),
        LegacyBasePolicy::RequireDeclared,
        &[slot("b"), slot("a")],
        2,
    )
    .unwrap();
    assert_eq!(prepared.base_tx, 6);
    for touched in [vec![], vec![slot("a")], vec![slot("a"), slot("c")]] {
        assert!(
            prepare_scoped_write_base(
                Some(&source),
                &current(),
                LegacyBasePolicy::RequireDeclared,
                &touched,
                2
            )
            .is_err()
        );
    }
    source.scope = None;
    assert!(
        prepare_scoped_write_base(
            Some(&source),
            &current(),
            LegacyBasePolicy::RequireDeclared,
            &[],
            2
        )
        .is_err()
    );
    source.scope = Some(vec![]);
    assert!(
        prepare_scoped_write_base(
            Some(&source),
            &current(),
            LegacyBasePolicy::RequireDeclared,
            &[],
            2
        )
        .is_ok()
    );
}

#[test]
fn scoped_preparation_bounds_legacy_and_rejects_duplicate_slots() {
    for touched in [
        vec![slot("a"), slot("a")],
        vec![slot("a"), slot("b"), slot("c")],
    ] {
        assert!(
            prepare_scoped_write_base(
                None,
                &current(),
                LegacyBasePolicy::UseCurrentAndFlag,
                &touched,
                2
            )
            .is_err()
        );
    }
    let mut source = declared();
    source.scope = Some(vec![slot("a"), slot("a")]);
    assert!(
        prepare_scoped_write_base(
            Some(&source),
            &current(),
            LegacyBasePolicy::RequireDeclared,
            &[slot("a")],
            2
        )
        .is_err()
    );
    assert!(
        prepare_scoped_write_base(
            None,
            &current(),
            LegacyBasePolicy::UseCurrentAndFlag,
            &[],
            0
        )
        .is_err()
    );
    let prepared = prepare_scoped_write_base(
        None,
        &current(),
        LegacyBasePolicy::UseCurrentAndFlag,
        &[slot("a")],
        1,
    )
    .unwrap();
    assert!(prepared.legacy_unbased);
    assert_eq!(prepared.base_tx, 10);
}

#[test]
fn prepared_reads_keep_independent_tx_cuts_and_explicit_valid_time() {
    let mut source = declared();
    source.scope = Some(vec![slot("a")]);
    let mut touched = vec![slot("a")];
    let prepared = prepare_slot_reads(
        Some(&source),
        &current(),
        LegacyBasePolicy::RequireDeclared,
        &touched,
        1,
        "2026-01-10T00:00:00Z",
    )
    .unwrap();
    touched.clear();
    assert_eq!(prepared.touched(), &[slot("a")]);
    assert_eq!(prepared.base().base_tx, 6);
    assert_eq!(prepared.base_cut().tx, Some(6));
    assert_eq!(prepared.current_cut().tx, Some(10));
    assert_eq!(
        prepared.base_cut().valid_at.as_deref(),
        Some("2026-01-10T00:00:00Z")
    );
    assert_eq!(
        prepared.current_cut().valid_at,
        prepared.base_cut().valid_at
    );
    let endpoint = prepare_slot_reads(
        Some(&source),
        &current(),
        LegacyBasePolicy::RequireDeclared,
        &[slot("a")],
        1,
        "2026-02-01T00:00:00Z",
    )
    .unwrap();
    assert_eq!(
        endpoint.base_cut().valid_at.as_deref(),
        Some("2026-02-01T00:00:00Z")
    );
}

#[test]
fn prepared_reads_refuse_implicit_time_and_unrepresentable_tx() {
    for time in ["", "now", "2026-01-10", "2026-01-10T01:00:00+01:00"] {
        assert!(
            prepare_slot_reads(
                None,
                &current(),
                LegacyBasePolicy::UseCurrentAndFlag,
                &[slot("a")],
                1,
                time
            )
            .is_err()
        );
    }
    let mut snapshot = current();
    snapshot.head_tx = u64::MAX;
    assert!(
        prepare_slot_reads(
            None,
            &snapshot,
            LegacyBasePolicy::UseCurrentAndFlag,
            &[slot("a")],
            1,
            "2026-01-10T00:00:00Z"
        )
        .is_err()
    );
}

#[test]
fn prepared_reads_capture_head_and_scope_without_borrowing_mutable_snapshot() {
    let mut snapshot = current();
    let mut source = declared();
    source.scope = Some(vec![slot("a")]);
    let mut touched = vec![slot("a")];
    let prepared = prepare_slot_reads(
        Some(&source),
        &snapshot,
        LegacyBasePolicy::RequireDeclared,
        &touched,
        1,
        "2026-02-01T00:00:00Z",
    )
    .unwrap();
    snapshot.head_tx = 99;
    snapshot.graph = "http://example.test/other".into();
    touched[0] = slot("replacement");
    assert_eq!(snapshot.head_tx, 99);
    assert_eq!(prepared.base_cut().tx, Some(6));
    assert_eq!(prepared.current_cut().tx, Some(10));
    assert_eq!(prepared.touched(), &[slot("a")]);
    assert_eq!(
        prepared.current_cut().valid_at.as_deref(),
        Some("2026-02-01T00:00:00Z")
    );
    assert_eq!(
        prepared.base().source.unwrap().graph,
        "http://example.test/graph"
    );
    // Capturing coordinates does not make a stale capture authoritative: the
    // integration must revalidate reservation and bindings before actual use.
}

#[test]
fn prepared_reads_preserve_exact_signed_store_boundary() {
    let mut snapshot = current();
    snapshot.head_tx = i64::MAX as u64;
    let mut source = declared();
    source.base_tx = snapshot.head_tx - 1;
    source.scope = Some(vec![slot("a")]);
    let prepared = prepare_slot_reads(
        Some(&source),
        &snapshot,
        LegacyBasePolicy::RequireDeclared,
        &[slot("a")],
        1,
        "2026-01-10T00:00:00Z",
    )
    .unwrap();
    assert_eq!(prepared.base_cut().tx, Some(i64::MAX - 1));
    assert_eq!(prepared.current_cut().tx, Some(i64::MAX));
    let legacy = prepare_slot_reads(
        None,
        &snapshot,
        LegacyBasePolicy::UseCurrentAndFlag,
        &[slot("a")],
        1,
        "2026-01-10T00:00:00Z",
    )
    .unwrap();
    assert_eq!(legacy.base_cut().tx, Some(i64::MAX));
    assert!(legacy.base().legacy_unbased);
    snapshot.head_tx += 1;
    assert!(
        prepare_slot_reads(
            Some(&source),
            &snapshot,
            LegacyBasePolicy::UseCurrentAndFlag,
            &[slot("a")],
            1,
            "2026-01-10T00:00:00Z"
        )
        .is_err()
    );
}

#[test]
fn prepared_reads_never_turn_bad_declared_scope_into_legacy_success() {
    for defect in 0..5 {
        let mut source = declared();
        source.scope = Some(vec![slot("a")]);
        match defect {
            0 => source.scope = None,
            1 => source.scope = Some(vec![slot("b")]),
            2 => source.scope = Some(vec![slot("a"), slot("a")]),
            3 => source.graph = "http://example.test/other".into(),
            _ => source.base_tx = 11,
        }
        assert!(
            prepare_slot_reads(
                Some(&source),
                &current(),
                LegacyBasePolicy::UseCurrentAndFlag,
                &[slot("a")],
                2,
                "2026-01-10T00:00:00Z"
            )
            .is_err(),
            "declared defect {defect} fell back"
        );
    }
    let malformed = crate::resolver_contract::Slot {
        subject: String::new(),
        predicate: "<http://example.test/p>".into(),
    };
    assert!(
        prepare_slot_reads(
            None,
            &current(),
            LegacyBasePolicy::UseCurrentAndFlag,
            &[malformed],
            1,
            "2026-01-10T00:00:00Z"
        )
        .is_err()
    );
}
