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
