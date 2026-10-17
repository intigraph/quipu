use super::*;
use crate::acquisition::composed::acquire;
use crate::{Op, Store, Value, store::Datum};

const MAIN: &str = "https://example.org/project/live";
const COLD: &str = "https://example.org/archive/cold";

fn fixture() -> (Payload, Request) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("main.db").to_str().unwrap()).unwrap();
    for (graph, text) in [(MAIN, "warm"), (COLD, "cold")] {
        let g = store.graph_create(graph).unwrap();
        let entity = store.intern("https://example.org/item").unwrap();
        let attribute = store.intern("https://example.org/text").unwrap();
        store
            .transact_to_graph(
                &[Datum {
                    entity,
                    attribute,
                    value: Value::Str(text.into()),
                    valid_from: "2026-01-01T00:00:00Z".into(),
                    valid_to: None,
                    op: Op::Assert,
                }],
                "2026-01-01T00:00:00Z",
                None,
                None,
                g,
            )
            .unwrap();
    }
    store
        .freeze_graph(
            COLD,
            dir.path().to_str().unwrap(),
            "2026-01-02T00:00:00Z",
            None,
        )
        .unwrap();
    let mut request = Request {
        source_store_id: store.store_id().unwrap(),
        graphs: vec![MAIN.into()],
        tx: None,
        expected_generation: None,
        max_bytes: None,
    };
    let first = acquire(&store, &request).unwrap();
    request.tx = Some(first.source_receipt.main.applied_tx);
    request.expected_generation = Some(first.source_receipt.composition.generation);
    let payload = acquire(&store, &request).unwrap();
    payload.verify(&request).unwrap();
    assert_eq!(payload.source_receipt.composition.providers.len(), 1);
    (payload, request)
}

fn copy(payload: &Payload) -> Payload {
    serde_json::from_value(serde_json::to_value(payload).unwrap()).unwrap()
}

fn request(payload: &Payload) -> Request {
    Request {
        source_store_id: payload.source_receipt.main.source_store_id.clone(),
        graphs: vec![MAIN.into()],
        tx: payload.source_receipt.main.requested_tx,
        expected_generation: Some(payload.source_receipt.composition.generation.clone()),
        max_bytes: None,
    }
}

// Counterfactual receipt mutations are restamped, not authenticated. This lets
// the tests distinguish cross-receipt comparison from native hash rejection.
fn restamp(payload: &mut Payload) {
    let r = &mut payload.source_receipt;
    let c = &mut r.composition;
    c.generation = crate::share::sha256(
        &serde_json::to_vec(&serde_json::json!([
            r.main.source_store_id,
            c.incarnation,
            c.providers,
            c.ownership_sha256
        ]))
        .unwrap(),
    );
    payload.source_receipt_sha256 =
        crate::share::sha256(&serde_json::to_vec(&serde_json::to_value(r).unwrap()).unwrap());
}

#[test]
fn native_frozen_provider_pair_matches_but_never_grants_readiness() {
    let (source, expected) = fixture();
    let result = compare(&source, &expected, &source, &expected).unwrap();
    assert!(!result.ready());
    let json = serde_json::to_value(result).unwrap();
    assert_eq!(json["ready"], false);
    assert_eq!(json["provider_count"], 1);
    assert_eq!(json["provider_closure"], "matched");
    assert_eq!(json["source_receipt_sha256"], source.source_receipt_sha256);
    assert_eq!(json["replica_receipt_sha256"], source.source_receipt_sha256);
}

#[test]
fn independent_physical_handles_are_not_a_shared_epoch() {
    let (source, expected) = fixture();
    let mut replica = copy(&source);
    replica.source_receipt.composition.incarnation = "other-process-incarnation".into();
    replica.source_receipt.composition.providers[0].instance = "other-fenced-inode".into();
    restamp(&mut replica);
    let replica_request = request(&replica);
    replica.verify(&replica_request).unwrap();
    let result = compare(&source, &expected, &replica, &replica_request).unwrap();
    assert!(!result.ready());
    assert_ne!(result.source_receipt_sha256, result.replica_receipt_sha256);
}

#[test]
fn physical_rebinding_preserves_matching_in_both_directions() {
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};

    let (source, expected) = fixture();
    let mut runner = TestRunner::new(Config {
        cases: 32,
        max_shrink_iters: 64,
        failure_persistence: None,
        ..Config::default()
    });
    runner
        .run(
            &("[a-z0-9]{1,32}", "[a-z0-9]{1,32}"),
            |(incarnation, instance)| {
                let mut replica = copy(&source);
                replica.source_receipt.composition.incarnation = incarnation;
                replica.source_receipt.composition.providers[0].instance = instance;
                restamp(&mut replica);
                let replica_request = request(&replica);
                prop_assert!(replica.verify(&replica_request).is_ok());
                let forward = compare(&source, &expected, &replica, &replica_request)
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                let reverse = compare(&replica, &replica_request, &source, &expected)
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                prop_assert!(!forward.ready() && !reverse.ready());
                prop_assert_eq!(forward.applied_tx, reverse.applied_tx);
                prop_assert_eq!(forward.provider_count, reverse.provider_count);
                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn missing_or_extra_provider_cannot_match_a_complete_native_census() {
    let (source, expected) = fixture();
    for extra in [false, true] {
        let mut replica = copy(&source);
        let providers = &mut replica.source_receipt.composition.providers;
        if extra {
            let mut second = providers[0].clone();
            second.alias = "extra".into();
            second.instance = "extra-handle".into();
            second.term_space += 1;
            providers.push(second);
        } else {
            providers.clear();
        }
        restamp(&mut replica);
        let replica_request = request(&replica);
        replica.verify(&replica_request).unwrap();
        assert!(compare(&source, &expected, &replica, &replica_request).is_err());
    }
}

#[test]
fn self_consistent_but_changed_portable_provider_identities_refuse() {
    type Change = fn(&mut Provider);
    let changes: [(&str, Change); 7] = [
        ("alias", |p| p.alias = "different".into()),
        ("lineage", |p| p.store_id = "different-lineage".into()),
        ("bytes", |p| {
            p.file_sha256 = format!("sha256:{}", "0".repeat(64));
        }),
        ("manifest", |p| {
            p.manifest_sha256 = format!("sha256:{}", "1".repeat(64));
        }),
        ("head", |p| p.head_tx += 1),
        ("term space", |p| p.term_space += 1),
        ("graph census", |p| {
            p.graphs = vec!["https://example.org/other".into()];
        }),
    ];
    let (source, expected) = fixture();
    for (name, change) in changes {
        let mut replica = copy(&source);
        change(&mut replica.source_receipt.composition.providers[0]);
        restamp(&mut replica);
        let replica_request = request(&replica);
        replica.verify(&replica_request).unwrap();
        assert!(
            compare(&source, &expected, &replica, &replica_request).is_err(),
            "{name}"
        );
    }
}

#[test]
fn independently_valid_different_main_lineage_refuses() {
    let (source, expected) = fixture();
    let mut replica = copy(&source);
    replica.source_receipt.main.source_store_id = "different-main-lineage".into();
    restamp(&mut replica);
    let replica_request = request(&replica);
    replica.verify(&replica_request).unwrap();
    assert!(compare(&source, &expected, &replica, &replica_request).is_err());
}

#[test]
fn independently_valid_different_policy_dependency_refuses() {
    let (source, expected) = fixture();
    let mut replica = copy(&source);
    replica.source_receipt.composition.ownership_sha256 = format!("sha256:{}", "2".repeat(64));
    restamp(&mut replica);
    let replica_request = request(&replica);
    replica.verify(&replica_request).unwrap();
    assert!(compare(&source, &expected, &replica, &replica_request).is_err());
}

#[test]
fn independently_valid_changed_main_bytes_refuse() {
    let (source, expected) = fixture();
    let mut replica = copy(&source);
    let text = replica.graphs.get_mut(MAIN).unwrap();
    *text = text.replace("\"warm\"", "\"changed\"");
    assert_ne!(text.as_str(), source.graphs[MAIN].as_str());
    replica
        .source_receipt
        .main
        .graphs
        .get_mut(MAIN)
        .unwrap()
        .sha256 = crate::share::sha256(text.as_bytes());
    restamp(&mut replica);
    let replica_request = request(&replica);
    replica.verify(&replica_request).unwrap();
    assert!(compare(&source, &expected, &replica, &replica_request).is_err());
}

#[test]
fn equal_present_reads_without_an_explicit_pin_do_not_match() {
    let (source, expected) = fixture();
    let mut unpinned = copy(&source);
    unpinned.source_receipt.main.requested_tx = None;
    restamp(&mut unpinned);
    let unpinned_request = request(&unpinned);
    unpinned.verify(&unpinned_request).unwrap();
    assert!(compare(&source, &expected, &unpinned, &unpinned_request).is_err());
    assert!(compare(&unpinned, &unpinned_request, &source, &expected).is_err());
}

#[test]
fn native_tamper_scope_and_generation_checks_are_not_bypassed() {
    let (source, expected) = fixture();
    let mut tampered = copy(&source);
    tampered.source_receipt_sha256 = "incorrect".into();
    assert!(compare(&source, &expected, &tampered, &expected).is_err());
    let mut bad_request = request(&source);
    bad_request.expected_generation = Some("wrong-generation".into());
    assert!(compare(&source, &expected, &source, &bad_request).is_err());
    bad_request = request(&source);
    bad_request.graphs.clear();
    assert!(compare(&source, &expected, &source, &bad_request).is_err());
    bad_request = request(&source);
    bad_request.tx = Some(source.source_receipt.main.applied_tx + 1);
    assert!(compare(&source, &expected, &source, &bad_request).is_err());
}

#[test]
fn incomplete_and_mutable_provider_receipts_refuse() {
    let (source, expected) = fixture();
    let mut incomplete = copy(&source);
    incomplete.source_receipt.complete = false;
    restamp(&mut incomplete);
    assert!(compare(&source, &expected, &incomplete, &request(&incomplete)).is_err());
    let mut changed = copy(&source);
    changed.source_receipt.composition.providers[0].exclusion = "included-without-contract".into();
    restamp(&mut changed);
    assert!(compare(&source, &expected, &changed, &request(&changed)).is_err());
}
