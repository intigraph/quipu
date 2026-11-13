//! Actual trust-root-gated scope enrollment; no carried key is an authority.
use super::*;
use crate::governance::{trust_root, verifier_registry};
use crate::store::Datum;
use crate::{Value, types::Op};

const AT: &str = "2026-01-01T00:00:00Z";
const LATER: &str = "2026-01-02T00:00:00Z";
const REG: &str = "urn:source-witness-registration";

fn source(graph: &str) -> Proof {
    let (store, graph, _, _) = super::tests::proof_and_grant(graph);
    seal(
        capture(
            &store,
            graph,
            "urn:fluent",
            "urn:event",
            &store.store_id().unwrap(),
        )
        .unwrap(),
        "urn:source-witness",
        &super::tests::signer(9),
    )
    .unwrap()
}

pub(super) fn registration(store: &mut Store, proof: &Proof, scope: &str, graph: i64) {
    let base = crate::namespace::DEFAULT_BASE_NS;
    let values = [
        (
            crate::namespace::RDF_TYPE.to_string(),
            Value::Ref(
                store
                    .intern(&format!("{base}VerifierRegistration"))
                    .unwrap(),
            ),
        ),
        (format!("{base}verifier"), Value::Str(proof.witness.clone())),
        (
            format!("{base}publicKey"),
            Value::Str(super::tests::signer(9).public_key_hex()),
        ),
        (format!("{base}attests"), Value::Str(scope.into())),
    ];
    let datums: Vec<_> = values
        .into_iter()
        .map(|(p, v)| Datum {
            entity: store.intern(REG).unwrap(),
            attribute: store.intern(&p).unwrap(),
            value: v,
            valid_from: AT.into(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    store
        .transact_to_graph(&datums, AT, None, None, graph)
        .unwrap();
}

pub(super) fn authorize(store: &mut Store, proof: &Proof) {
    registration(store, proof, &proof.scope_id().unwrap(), 0);
    trust_root::test_support::govern(store, REG, verifier_registry::AGENT_TIER, AT);
}

#[test]
fn exact_governed_root_scope_admits_but_raw_or_generic_registrations_do_not() {
    let proof = source(crate::schema::ROOT_GRAPH_IRI);
    let mut receiver = Store::open_in_memory_test_ns().unwrap();
    registration(&mut receiver, &proof, &proof.scope_id().unwrap(), 0);
    // The raw registry sees the real key; it still confers no governed authority.
    assert_eq!(
        verifier_registry::registered_keys(
            &receiver,
            &proof.witness,
            Some(&proof.scope_id().unwrap()),
            &verifier_registry::Witness::now(),
            verifier_registry::Scope::Root
        )
        .unwrap()
        .len(),
        1
    );
    let before = receiver.latest_tx_id().unwrap();
    assert!(proof.verify_governed_now(&receiver).is_err());
    assert_eq!(before, receiver.latest_tx_id().unwrap());
    trust_root::test_support::govern(&mut receiver, REG, verifier_registry::AGENT_TIER, AT);
    let verified = proof.verify_governed_now(&receiver).unwrap();
    assert_eq!(verified.observation, proof.body.observation);
    let mut generic = Store::open_in_memory_test_ns().unwrap();
    registration(&mut generic, &proof, "quipu-share-v1", 0);
    trust_root::test_support::govern(&mut generic, REG, verifier_registry::AGENT_TIER, AT);
    assert!(proof.verify_governed_now(&generic).is_err());
}

#[test]
fn all_declaration_fields_and_profile_participate_in_the_governed_scope() {
    let proof = source(crate::schema::ROOT_GRAPH_IRI);
    let mut receiver = Store::open_in_memory_test_ns().unwrap();
    authorize(&mut receiver, &proof);
    for field in 0..8 {
        let mut changed = proof.clone();
        match field {
            0 => changed.body.source_store = "urn:other-source".into(),
            1 => changed.body.slot.graph = "urn:other-graph".into(),
            2 => {
                changed.body.slot.fluent = "urn:other-fluent".into();
                changed.body.observation.fluent = "urn:other-fluent".into();
            }
            3 => changed.body.slot.subject = "urn:other-subject".into(),
            4 => changed.body.slot.predicate = "urn:other-predicate".into(),
            5 => changed.body.slot.property_shape = "urn:other-shape".into(),
            6 => changed.body.slot.policy = Policy::Max,
            _ => changed.body.profile_sha256 = "0".repeat(64),
        }
        assert_ne!(changed.scope_id().unwrap(), proof.scope_id().unwrap());
        // A valid source-witness signature cannot enlarge its governed grant.
        changed.signature =
            super::tests::signer(9).sign(&message(&changed.body, &changed.witness).unwrap());
        assert!(
            changed.verify_governed_now(&receiver).is_err(),
            "field {field}"
        );
    }
    assert!(proof.verify_governed_now(&receiver).is_ok());
}

#[test]
fn revocation_is_current_and_sender_recording_time_cannot_backdate_authority() {
    let mut proof = source(crate::schema::ROOT_GRAPH_IRI);
    let mut receiver = Store::open_in_memory_test_ns().unwrap();
    authorize(&mut receiver, &proof);
    assert!(proof.verify_governed_now(&receiver).is_ok());
    trust_root::test_support::amend_retract(
        &mut receiver,
        REG,
        &format!("{}attests", crate::namespace::DEFAULT_BASE_NS),
        Value::Str(proof.scope_id().unwrap()),
        AT,
        LATER,
    );
    assert!(proof.verify_governed_now(&receiver).is_err());
    proof.body.observation.recorded_on = AT.into();
    proof.signature = super::tests::signer(9).sign(&message(&proof.body, &proof.witness).unwrap());
    assert!(proof.verify_governed_now(&receiver).is_err());
}

#[test]
fn named_graph_registrations_never_supply_root_authority_for_any_data_graph() {
    for graph in [crate::schema::ROOT_GRAPH_IRI, "urn:graph-a", "urn:graph-b"] {
        let proof = source(graph);
        let mut receiver = Store::open_in_memory_test_ns().unwrap();
        let named = receiver.graph_create("urn:untrusted-registry").unwrap();
        registration(&mut receiver, &proof, &proof.scope_id().unwrap(), named);
        let marker = Datum {
            entity: receiver.intern(REG).unwrap(),
            attribute: receiver.intern(verifier_registry::TRUST_TIER).unwrap(),
            value: Value::Str(verifier_registry::AGENT_TIER.into()),
            valid_from: AT.into(),
            valid_to: None,
            op: Op::Assert,
        };
        receiver
            .transact_to_graph(&[marker], AT, None, None, named)
            .unwrap();
        assert_eq!(
            verifier_registry::registered_keys(
                &receiver,
                &proof.witness,
                Some(&proof.scope_id().unwrap()),
                &verifier_registry::Witness::now(),
                verifier_registry::Scope::AllGraphs
            )
            .unwrap()
            .len(),
            1
        );
        assert!(proof.verify_governed_now(&receiver).is_err());
        authorize(&mut receiver, &proof);
        assert!(proof.verify_governed_now(&receiver).is_ok());
    }
}
