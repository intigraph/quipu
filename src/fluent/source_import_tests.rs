//! Real closed native writer admission; imported authentication stays distinct.
use super::*;
use crate::transaction_auth::{Identity, with_identity};

fn identity() -> Identity {
    Identity {
        principal: "urn:receiver-importer".into(),
        credential_id: Some("receiver-bearer-id".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}

fn fixtures(graph: &str) -> (Store, i64, Proof) {
    let (source, g) = super::tests::loaded_event(graph, "urn:source-event");
    let proof = seal(
        capture(
            &source,
            g,
            "urn:fluent",
            "urn:source-event",
            &source.store_id().unwrap(),
        )
        .unwrap(),
        "urn:source-witness",
        &super::tests::signer(9),
    )
    .unwrap();
    let (mut receiver, g) = super::tests::loaded_event(graph, "urn:receiver-event");
    receiver.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
    super::governed_tests::authorize(&mut receiver, &proof);
    (receiver, g, proof)
}

fn counts(store: &Store, graph: i64) -> (i64, usize, i64, i64) {
    (
        store.latest_tx_id().unwrap(),
        store.current_facts_in_graph(graph).unwrap().len(),
        store
            .conn
            .query_row("SELECT COUNT(*) FROM terms", [], |r| r.get(0))
            .unwrap(),
        store
            .conn
            .query_row("SELECT COUNT(*) FROM fluent_origin_receipts", [], |r| {
                r.get(0)
            })
            .unwrap(),
    )
}

#[test]
fn import_retains_remote_order_and_proof_but_assigns_local_import_auth_and_replay_tx() {
    for graph in [crate::schema::ROOT_GRAPH_IRI, "urn:graph-a", "urn:graph-b"] {
        let (mut receiver, g, proof) = fixtures(graph);
        let tx = with_identity(Some(identity()), || {
            receiver.import_fluent_native(&proof, g)
        })
        .unwrap();
        let auth = receiver.transaction_auth(tx).unwrap().unwrap();
        assert_eq!(auth.principal, "urn:receiver-importer");
        assert_eq!(auth.credential_id.as_deref(), Some("receiver-bearer-id"));
        let log = writer::read_log(&receiver, g, "urn:fluent", Policy::Sum).unwrap();
        assert!(
            log.observations("urn:fluent")
                .contains(&&proof.body.observation)
        );
        let event = receiver.lookup("urn:source-event").unwrap().unwrap();
        let (bytes, admission) = receiver.fluent_origin_receipt(g, event).unwrap().unwrap();
        assert_eq!(admission, tx);
        assert_eq!(bytes, serde_json::to_vec(&proof).unwrap());
        let before = counts(&receiver, g);
        assert_eq!(
            with_identity(Some(identity()), || receiver
                .import_fluent_native(&proof, g))
            .unwrap(),
            tx
        );
        assert_eq!(counts(&receiver, g), before);
        // A receiver witness cannot launder transported origin into local origin.
        assert!(
            capture(
                &receiver,
                g,
                "urn:fluent",
                "urn:source-event",
                &receiver.store_id().unwrap()
            )
            .is_err()
        );
        assert_eq!(counts(&receiver, g), before);
    }
}

#[test]
fn import_missing_identity_wrong_graph_profile_and_forged_proof_roll_back_atomically() {
    let (mut receiver, g, proof) = fixtures(crate::schema::ROOT_GRAPH_IRI);
    let before = counts(&receiver, g);
    assert!(receiver.import_fluent_native(&proof, g).is_err());
    assert_eq!(counts(&receiver, g), before);
    let mut forged = proof.clone();
    forged.body.observation.order.writer = "urn:forged-actor".into();
    assert!(
        with_identity(Some(identity()), || receiver
            .import_fluent_native(&forged, g))
        .is_err()
    );
    assert_eq!(counts(&receiver, g), before);
    let wrong = receiver.graph_create("urn:wrong-graph").unwrap();
    let before = counts(&receiver, wrong);
    assert!(
        with_identity(Some(identity()), || receiver
            .import_fluent_native(&proof, wrong))
        .is_err()
    );
    assert_eq!(counts(&receiver, wrong), before);
    receiver
        .load_shapes(
            "extra",
            "<urn:extra> <http://www.w3.org/ns/shacl#path> <urn:extra-path> .",
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
    let before = counts(&receiver, g);
    assert!(
        with_identity(Some(identity()), || receiver
            .import_fluent_native(&proof, g))
        .is_err()
    );
    assert_eq!(counts(&receiver, g), before);
}

#[test]
fn source_receipts_cannot_be_forged_updated_or_deleted_without_native_permits() {
    let (mut receiver, g, proof) = fixtures(crate::schema::ROOT_GRAPH_IRI);
    let event = receiver.intern("urn:source-event").unwrap();
    let before = counts(&receiver, g);
    assert!(
        receiver
            .persist_fluent_origin(
                g,
                event,
                receiver.latest_tx_id().unwrap(),
                &serde_json::to_vec(&proof).unwrap()
            )
            .is_err()
    );
    assert_eq!(counts(&receiver, g), before);
    with_identity(Some(identity()), || {
        receiver.import_fluent_native(&proof, g)
    })
    .unwrap();
    let before = counts(&receiver, g);
    assert!(
        receiver
            .conn
            .execute("UPDATE fluent_origin_receipts SET proof=X'00'", [])
            .is_err()
    );
    assert!(
        receiver
            .conn
            .execute("DELETE FROM fluent_origin_receipts", [])
            .is_err()
    );
    assert_eq!(counts(&receiver, g), before);
    assert!(writer::read_log(&receiver, g, "urn:fluent", Policy::Sum).is_ok());
}

#[test]
fn revocation_after_precheck_refuses_new_import_without_using_sender_time() {
    let (mut receiver, g, proof) = fixtures(crate::schema::ROOT_GRAPH_IRI);
    assert!(proof.verify_governed_now(&receiver).is_ok());
    crate::governance::trust_root::test_support::amend_retract(
        &mut receiver,
        "urn:source-witness-registration",
        &format!("{}attests", crate::namespace::DEFAULT_BASE_NS),
        crate::Value::Str(proof.scope_id().unwrap()),
        "2026-01-01T00:00:00Z",
        "2026-01-02T00:00:00Z",
    );
    let before = counts(&receiver, g);
    assert!(
        with_identity(Some(identity()), || receiver
            .import_fluent_native(&proof, g))
        .is_err()
    );
    assert_eq!(counts(&receiver, g), before);
}

#[test]
fn imported_origin_reopens_on_disk_with_unchanged_proof_and_independent_sql_guards() {
    let (mut receiver, g, proof) = fixtures("urn:graph-a");
    with_identity(Some(identity()), || {
        receiver.import_fluent_native(&proof, g)
    })
    .unwrap();
    let before = writer::read_log(&receiver, g, "urn:fluent", Policy::Sum).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipt.db");
    std::fs::write(&path, receiver.serialize_db().unwrap()).unwrap();
    drop(receiver);
    let raw = rusqlite::Connection::open(&path).unwrap();
    assert!(
        raw.execute("UPDATE fluent_origin_receipts SET tx=tx+1", [])
            .is_err()
    );
    assert!(
        raw.execute("DELETE FROM fluent_origin_receipts", [])
            .is_err()
    );
    drop(raw);
    let mut reopened = Store::open(path.to_str().unwrap()).unwrap();
    reopened.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
    assert_eq!(
        writer::read_log(&reopened, g, "urn:fluent", Policy::Sum).unwrap(),
        before
    );
    assert_eq!(
        serde_json::to_vec(&Proof::retained(&reopened, g, "urn:source-event").unwrap()).unwrap(),
        serde_json::to_vec(&proof).unwrap()
    );
}

#[test]
fn relay_requires_independent_receiver_grant_and_retains_original_source_identity() {
    let (mut receiver, g, proof) = fixtures("urn:graph-a");
    with_identity(Some(identity()), || {
        receiver.import_fluent_native(&proof, g)
    })
    .unwrap();
    let forwarded = Proof::retained(&receiver, g, "urn:source-event").unwrap();
    assert_eq!(
        serde_json::to_vec(&forwarded).unwrap(),
        serde_json::to_vec(&proof).unwrap()
    );
    let (mut third, g) = super::tests::loaded_event("urn:graph-a", "urn:third-event");
    third.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
    let before = counts(&third, g);
    assert!(
        with_identity(Some(identity()), || third
            .import_fluent_native(&forwarded, g))
        .is_err()
    );
    assert_eq!(counts(&third, g), before);
    super::governed_tests::authorize(&mut third, &proof);
    with_identity(Some(identity()), || {
        third.import_fluent_native(&forwarded, g)
    })
    .unwrap();
    let final_proof = Proof::retained(&third, g, "urn:source-event").unwrap();
    assert_eq!(final_proof.body.source_store, proof.body.source_store);
    assert_ne!(final_proof.body.source_store, receiver.store_id().unwrap());
    assert_ne!(final_proof.body.source_store, third.store_id().unwrap());
    assert_eq!(
        serde_json::to_vec(&final_proof).unwrap(),
        serde_json::to_vec(&proof).unwrap()
    );
}

#[test]
fn receipt_history_survives_later_revocation_but_retroactive_compromise_refuses_reads() {
    for retroactive in [false, true] {
        let (mut receiver, g, proof) = fixtures(crate::schema::ROOT_GRAPH_IRI);
        with_identity(Some(identity()), || {
            receiver.import_fluent_native(&proof, g)
        })
        .unwrap();
        let before = writer::read_log(&receiver, g, "urn:fluent", Policy::Sum).unwrap();
        let end = if retroactive {
            "2026-01-02T00:00:00Z".to_string()
        } else {
            // Derive the boundary from the actual admission, not the wall date
            // on which this test happens to run.
            let admission = receiver.latest_tx_id().unwrap();
            let tx = receiver.get_transaction(admission).unwrap().unwrap();
            let epoch = crate::time::epoch_of_rfc3339(&tx.timestamp).unwrap();
            crate::time::format_iso(u64::try_from(epoch).unwrap().checked_add(1).unwrap())
        };
        crate::governance::trust_root::test_support::amend_retract(
            &mut receiver,
            "urn:source-witness-registration",
            &format!("{}attests", crate::namespace::DEFAULT_BASE_NS),
            crate::Value::Str(proof.scope_id().unwrap()),
            "2026-01-01T00:00:00Z",
            &end,
        );
        assert!(proof.verify_governed_now(&receiver).is_err());
        let snapshot = counts(&receiver, g);
        assert!(
            with_identity(Some(identity()), || receiver
                .import_fluent_native(&proof, g))
            .is_err()
        );
        assert_eq!(counts(&receiver, g), snapshot);
        let after = writer::read_log(&receiver, g, "urn:fluent", Policy::Sum);
        if !retroactive {
            assert_eq!(after.unwrap(), before);
        } else {
            assert!(after.is_err());
        }
        // Refusal never erases or rewrites the retained historical records.
        assert_eq!(counts(&receiver, g), snapshot);
    }
}

#[test]
fn populated_native_records_and_origin_receipts_survive_respace_without_source_mutation() {
    let (mut receiver, g, proof) = fixtures("urn:graph-a");
    with_identity(Some(identity()), || {
        receiver.import_fluent_native(&proof, g)
    })
    .unwrap();
    let expected = writer::read_log(&receiver, g, "urn:fluent", Policy::Sum).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source.db");
    let dst = dir.path().join("respaced.db");
    let original = receiver.serialize_db().unwrap();
    std::fs::write(&src, &original).unwrap();
    drop(receiver);
    let result = crate::store::respace::respace_file(&src, &dst, 1);
    assert_eq!(std::fs::read(&src).unwrap(), original);
    result.unwrap();
    // Inspect the committed output before Store::open can reinstall anything.
    let raw = rusqlite::Connection::open(&dst).unwrap();
    assert!(
        raw.execute("DELETE FROM fluent_origin_receipts", [])
            .is_err()
    );
    assert!(raw.execute("UPDATE facts SET valid_to='2027-01-01T00:00:00Z' WHERE e IN (SELECT e FROM fluent_origin_receipts)", []).is_err());
    drop(raw);
    let mut reopened = Store::open(dst.to_str().unwrap()).unwrap();
    reopened.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
    let g = reopened.lookup("urn:graph-a").unwrap().unwrap();
    assert_eq!(
        writer::read_log(&reopened, g, "urn:fluent", Policy::Sum).unwrap(),
        expected
    );
    let event = reopened.lookup("urn:source-event").unwrap().unwrap();
    assert!(
        reopened
            .conn
            .execute("DELETE FROM fluent_origin_receipts WHERE e=?1", [event])
            .is_err()
    );
}

#[test]
fn first_remote_event_adopts_matching_local_descriptor_without_artificial_observations() {
    for graph in [crate::schema::ROOT_GRAPH_IRI, "urn:graph-a", "urn:graph-b"] {
        let (source, g) = super::tests::loaded_event(graph, "urn:source-first-event");
        let proof = seal(
            capture(
                &source,
                g,
                "urn:fluent",
                "urn:source-first-event",
                &source.store_id().unwrap(),
            )
            .unwrap(),
            "urn:source-witness",
            &super::tests::signer(9),
        )
        .unwrap();
        let (mut receiver, g) = super::tests::loaded_shapes(graph);
        receiver.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
        super::governed_tests::authorize(&mut receiver, &proof);
        assert_eq!(receiver.lookup("urn:fluent").unwrap(), None);
        let before = counts(&receiver, g);
        assert!(receiver.import_fluent_native(&proof, g).is_err());
        assert_eq!(counts(&receiver, g), before);
        let tx = with_identity(Some(identity()), || {
            receiver.import_fluent_native(&proof, g)
        })
        .unwrap();
        let (_, log) = native_registry::load(&receiver, g).unwrap();
        assert_eq!(
            log.observations("urn:fluent"),
            vec![&proof.body.observation]
        );
        assert_eq!(
            log.current("urn:fluent", Policy::Sum)
                .unwrap()
                .unwrap()
                .values,
            std::collections::BTreeSet::from([crate::fluent::Scalar::Integer(3)])
        );
        let fluent = receiver.lookup("urn:fluent").unwrap().unwrap();
        let descriptor = receiver.entity_facts_in_graph(fluent, g).unwrap();
        assert_eq!(descriptor.len(), 6);
        assert!(descriptor.iter().all(|fact| fact.tx == tx));
        assert_eq!(
            receiver.transaction_auth(tx).unwrap().unwrap().principal,
            "urn:receiver-importer"
        );
        assert_ne!(proof.body.observation.order.writer, "urn:receiver-importer");
        let before = counts(&receiver, g);
        assert_eq!(
            with_identity(Some(identity()), || receiver
                .import_fluent_native(&proof, g))
            .unwrap(),
            tx
        );
        assert_eq!(counts(&receiver, g), before);
    }
}
