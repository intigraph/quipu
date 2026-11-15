use super::*;
use crate::fluent::{Scalar, writer::Append};
use crate::transaction_auth::{Identity, with_identity};

fn identity() -> Identity {
    Identity {
        principal: "urn:batch-importer".into(),
        credential_id: Some("batch-fixture".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}

fn source(graph: &str, label: &str, values: &[i64]) -> Vec<Proof> {
    let (mut store, g) = super::tests::loaded_shapes(graph);
    let mut proofs = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let request = Append {
            fluent: "urn:fluent".into(),
            event: format!("urn:event:{label}:{index}"),
            subject: "urn:subject".into(),
            predicate: "urn:count".into(),
            property_shape: "urn:shape".into(),
            as_of: "2026-11-08T00:00:00Z".into(),
            value: Scalar::Integer(*value),
        };
        with_identity(Some(identity()), || store.append_fluent_native(&request, g)).unwrap();
        proofs.push(
            seal(
                capture(
                    &store,
                    g,
                    "urn:fluent",
                    &request.event,
                    &store.store_id().unwrap(),
                )
                .unwrap(),
                "urn:source-witness",
                &super::tests::signer(9),
            )
            .unwrap(),
        );
    }
    proofs
}

fn receiver(graph: &str, proofs: &[Proof]) -> (Store, i64) {
    let (mut store, g) = super::tests::loaded_shapes(graph);
    store.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
    // Enroll all exact scopes before the real signed trust-root endorsement.
    for proof in proofs {
        super::governed_tests::registration(&mut store, proof, &proof.scope_id().unwrap(), 0);
    }
    crate::governance::trust_root::test_support::govern(
        &mut store,
        "urn:source-witness-registration",
        crate::governance::verifier_registry::AGENT_TIER,
        "2026-01-01T00:00:00Z",
    );
    (store, g)
}

fn counts(store: &Store, g: i64) -> (i64, usize, i64, i64) {
    (
        store.latest_tx_id().unwrap(),
        store.current_facts_in_graph(g).unwrap().len(),
        store
            .conn
            .query_row("SELECT count(*) FROM terms", [], |r| r.get(0))
            .unwrap(),
        store
            .conn
            .query_row("SELECT count(*) FROM fluent_origin_receipts", [], |r| {
                r.get(0)
            })
            .unwrap(),
    )
}

#[test]
fn native_union_batch_is_commutative_idempotent_and_one_local_admission() {
    for graph in [crate::schema::ROOT_GRAPH_IRI, "urn:graph-a", "urn:graph-b"] {
        let proofs = source(graph, "union", &[3, 5]);
        let mut logs = Vec::new();
        for order in [[0, 1, 0], [1, 0, 1]] {
            let (mut store, g) = receiver(graph, &proofs);
            let packet: Vec<_> = order.iter().map(|n| proofs[*n].clone()).collect();
            let tx = with_identity(Some(identity()), || {
                store.import_fluent_batch_native(&packet, g)
            })
            .unwrap();
            let log = writer::read_log(&store, g, "urn:fluent", Policy::Sum).unwrap();
            assert_eq!(log.observations("urn:fluent").len(), 2);
            assert_eq!(
                log.current("urn:fluent", Policy::Sum)
                    .unwrap()
                    .unwrap()
                    .values,
                std::collections::BTreeSet::from([Scalar::Integer(8)])
            );
            for proof in &proofs {
                let event = store
                    .lookup(&proof.body.observation.order.event)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    store.fluent_origin_receipt(g, event).unwrap().unwrap().1,
                    tx
                );
            }
            let before = counts(&store, g);
            assert_eq!(
                with_identity(Some(identity()), || store
                    .import_fluent_batch_native(&packet, g))
                .unwrap(),
                tx
            );
            assert_eq!(counts(&store, g), before);
            logs.push(log);
        }
        assert_eq!(logs[0], logs[1]);
    }
}

#[test]
fn invalid_or_conflicting_last_event_rolls_back_whole_native_batch() {
    let proofs = source("urn:graph-a", "atomic", &[3, 5]);
    for conflict in [false, true] {
        let (mut store, g) = receiver("urn:graph-a", &proofs);
        let mut packet = proofs.clone();
        if conflict {
            packet[1] = proofs[0].clone();
            packet[1].body.observation.value = Scalar::Integer(9);
            packet[1].signature = super::tests::signer(9)
                .sign(&message(&packet[1].body, &packet[1].witness).unwrap());
            assert!(packet[1].verify_governed_now(&store).is_ok());
        } else {
            packet[1].signature = "0".repeat(128);
        }
        let before = counts(&store, g);
        assert!(
            with_identity(Some(identity()), || store
                .import_fluent_batch_native(&packet, g))
            .is_err()
        );
        assert_eq!(counts(&store, g), before);
        assert_eq!(store.lookup("urn:fluent").unwrap(), None);
        assert_eq!(
            store
                .lookup(&proofs[0].body.observation.order.event)
                .unwrap(),
            None
        );
    }
}

#[test]
fn individually_valid_sources_cannot_overflow_the_combined_union() {
    let mut proofs = source("urn:graph-a", "maximum", &[i64::MAX]);
    proofs.extend(source("urn:graph-a", "extra", &[1]));
    let (mut store, g) = receiver("urn:graph-a", &proofs);
    for proof in &proofs {
        assert!(proof.verify_governed_now(&store).is_ok());
    }
    let before = counts(&store, g);
    assert!(
        with_identity(Some(identity()), || store
            .import_fluent_batch_native(&proofs, g))
        .is_err()
    );
    assert_eq!(counts(&store, g), before);
    assert_eq!(store.lookup("urn:fluent").unwrap(), None);
}

#[test]
fn empty_and_oversized_native_packets_refuse_without_allocation() {
    let proofs = source("urn:graph-a", "bounded", &[3]);
    let (mut store, g) = receiver("urn:graph-a", &proofs);
    for packet in [Vec::new(), vec![proofs[0].clone(); 65]] {
        let before = counts(&store, g);
        assert!(
            with_identity(Some(identity()), || store
                .import_fluent_batch_native(&packet, g))
            .is_err()
        );
        assert_eq!(counts(&store, g), before);
    }
}
