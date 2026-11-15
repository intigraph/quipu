use super::*;
use crate::fluent::{Scalar, writer::Append};
use crate::transaction_auth::{Identity, with_identity};
use ring::signature::Ed25519KeyPair;
use serde_json::json;

pub(super) fn signer(seed: u8) -> quipu_signing::SigningIdentity {
    quipu_signing::SigningIdentity::new(
        Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap(),
        "fixture",
    )
}
fn loaded(graph_name: &str) -> (Store, i64) {
    loaded_event(graph_name, "urn:event")
}

pub(super) fn loaded_shapes(graph_name: &str) -> (Store, i64) {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes(
            "quipu-fluents-v1",
            include_str!("../../shapes/fluents-v1.ttl"),
            "2026-11-08T00:00:00Z",
        )
        .unwrap();
    store.load_shapes("declarations", "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix qp: <https://w3id.org/intigraph/quipu/ns#> . <urn:shape> sh:path <urn:count>; sh:targetNode <urn:subject>; qp:currentValuePolicy \"sum\" .", "2026-11-08T00:00:00Z").unwrap();
    let graph = if graph_name == crate::schema::ROOT_GRAPH_IRI {
        0
    } else {
        store.graph_create(graph_name).unwrap()
    };
    (store, graph)
}

pub(super) fn loaded_event(graph_name: &str, event: &str) -> (Store, i64) {
    let (mut store, graph) = loaded_shapes(graph_name);
    let identity = Identity {
        principal: "urn:actual-writer".into(),
        credential_id: Some("fixture-bearer-id".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    };
    let request = Append {
        fluent: "urn:fluent".into(),
        event: event.into(),
        subject: "urn:subject".into(),
        predicate: "urn:count".into(),
        property_shape: "urn:shape".into(),
        as_of: "2026-11-08T00:00:00Z".into(),
        value: Scalar::Integer(3),
    };
    with_identity(Some(identity), || {
        store.append_fluent_native(&request, graph)
    })
    .unwrap();
    (store, graph)
}
pub(super) fn proof_and_grant(graph: &str) -> (Store, i64, Proof, WitnessGrant) {
    let (store, id) = loaded(graph);
    let key = signer(7);
    let origin = capture(
        &store,
        id,
        "urn:fluent",
        "urn:event",
        &store.store_id().unwrap(),
    )
    .unwrap();
    let proof = seal(origin, "urn:source-witness", &key).unwrap();
    let grant = WitnessGrant {
        witness: proof.witness.clone(),
        source_store: proof.body.source_store.clone(),
        slot: proof.body.slot.clone(),
        profile_sha256: proof.body.profile_sha256.clone(),
        public_key_hex: key.public_key_hex(),
        revoked: false,
    };
    (store, id, proof, grant)
}

#[test]
fn bearer_origin_roundtrips_without_a_per_event_writer_signature_or_source_mutation() {
    let (store, _, proof, grant) = proof_and_grant(crate::schema::ROOT_GRAPH_IRI);
    let tx = store.latest_tx_id().unwrap();
    let facts = store.current_facts_in_graph(0).unwrap().len();
    let bytes = serde_json::to_vec(&proof).unwrap();
    let decoded = Proof::decode(&bytes).unwrap();
    let verified = decoded
        .verify(&[grant], crate::schema::ROOT_GRAPH_IRI, "urn:fluent")
        .unwrap();
    assert_eq!(verified.observation, proof.body.observation);
    assert_eq!(verified.original_proof.signature, proof.signature);
    assert_eq!(verified.original_proof.body.auth_class, "named_bearer");
    assert_eq!(
        verified.original_proof.body.credential_id.as_deref(),
        Some("fixture-bearer-id")
    );
    assert_eq!(tx, store.latest_tx_id().unwrap());
    assert_eq!(facts, store.current_facts_in_graph(0).unwrap().len());
}

#[test]
fn actor_and_native_source_text_do_not_let_the_witness_capture_unadmitted_records() {
    let (mut store, _) = loaded(crate::schema::ROOT_GRAPH_IRI);
    let qp = crate::namespace::QUIPU_FLUENT;
    let turtle = format!(
        "<urn:forged> <{qp}forFluent> <urn:fluent>; <{qp}writer> <urn:actual-writer>; <{qp}originTx> 1; <{qp}observationValue> 999 ."
    );
    // Ordinary input is a positive control, but its actor/source strings are not proof.
    crate::rdf::ingest_rdf(
        &mut store,
        turtle.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-11-08T00:00:00Z",
        Some("urn:actual-writer"),
        Some("quipu:fluent"),
    )
    .unwrap();
    let before = store.latest_tx_id().unwrap();
    let source = store.store_id().unwrap();
    assert!(capture(&store, 0, "urn:fluent", "urn:forged", &source).is_err());
    assert!(capture(&store, 0, "urn:fluent", "urn:event", &source).is_ok());
    assert_eq!(before, store.latest_tx_id().unwrap());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn genuine_bundle_signature_is_not_a_source_witness_grant_or_signature_domain() {
    let (_, _, proof, grant) = proof_and_grant(crate::schema::ROOT_GRAPH_IRI);
    assert!(
        proof
            .verify(&[], crate::schema::ROOT_GRAPH_IRI, "urn:fluent")
            .is_err()
    );
    let mut generic = proof.clone();
    let key = signer(7);
    let mut ordinary = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut ordinary);
    crate::rdf::ingest_rdf(
        &mut ordinary,
        b"<urn:ordinary> <urn:count> 3 .".as_slice(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-11-08T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    let payload = crate::share::share_payload(
        &ordinary,
        &crate::share::ShareOptions {
            no_shapes: true,
            ..Default::default()
        },
        MAX_PROOF,
    )
    .unwrap();
    use crate::session_attestation::{
        self as attest, AttestationEnvelope, SessionBinding, ShareBinding, SignedBinding,
    };
    let producer = SessionBinding::new(
        "urn:producer",
        "urn:producer-session",
        key.public_key_hex(),
        "urn:introducer",
        100,
        200,
    )
    .unwrap();
    let mut envelope = AttestationEnvelope {
        version: attest::SHARE_V1.into(),
        key_id: producer.key_id.clone(),
        session: producer.session.clone(),
        introducer: producer.introducer.clone(),
        issued_at_epoch: 100,
        nonce: "0".repeat(32),
        signature: String::new(),
        audience: None,
    };
    let binding = SignedBinding::Share(ShareBinding {
        share_id: &payload.manifest.share_id,
        graph_hash: &payload.manifest.graph_hash,
        shapes_hash: &payload.manifest.shapes_hash,
        tx_anchor: payload.manifest.tx_anchor,
    });
    envelope.signature = key.sign(&attest::canonical_message(&envelope, &binding));
    let producers = attest::BindingRegistry::default();
    producers.register(producer).unwrap();
    // Actual shipped generic-share verification positively trusts this producer.
    assert_eq!(
        producers.verify(&envelope, &binding, 100, 0).unwrap().agent,
        "urn:producer"
    );
    // Its genuine signature still cannot substitute for event-origin evidence.
    generic.signature = envelope.signature;
    assert!(
        generic
            .verify(&[grant], crate::schema::ROOT_GRAPH_IRI, "urn:fluent")
            .is_err()
    );
}

#[test]
fn root_named_and_two_named_scope_inverses_refuse_while_matching_scopes_verify() {
    for source in [crate::schema::ROOT_GRAPH_IRI, "urn:graph-a", "urn:graph-b"] {
        let (_, _, proof, grant) = proof_and_grant(source);
        for target in [crate::schema::ROOT_GRAPH_IRI, "urn:graph-a", "urn:graph-b"] {
            assert_eq!(
                proof
                    .verify(std::slice::from_ref(&grant), target, "urn:fluent")
                    .is_ok(),
                source == target
            );
            let mut false_grant = grant.clone();
            false_grant.slot.graph = target.into();
            assert_eq!(
                proof.verify(&[false_grant], source, "urn:fluent").is_ok(),
                source == target
            );
        }
    }
}

#[test]
fn changed_original_fields_revocation_wrong_keys_and_ambiguous_grants_refuse() {
    let (_, _, proof, grant) = proof_and_grant(crate::schema::ROOT_GRAPH_IRI);
    let mut variants = Vec::new();
    for field in 0..9 {
        let mut changed = proof.clone();
        match field {
            0 => changed.body.observation.order.tx += 1,
            1 => changed.body.observation.order.writer = "urn:forged-writer".into(),
            2 => changed.body.observation.value = Scalar::Integer(999),
            3 => changed.body.observation.as_of = "2026-11-09T00:00:00Z".into(),
            4 => changed.body.observation.recorded_on = "2026-11-09T00:00:00Z".into(),
            5 => changed.body.source_store = "urn:other-source".into(),
            6 => changed.body.profile_sha256 = "0".repeat(64),
            7 => changed.body.slot.property_shape = "urn:other-shape".into(),
            _ => changed.witness = "urn:other-witness".into(),
        }
        variants.push(changed);
    }
    for changed in variants {
        assert!(
            changed
                .verify(
                    std::slice::from_ref(&grant),
                    crate::schema::ROOT_GRAPH_IRI,
                    "urn:fluent"
                )
                .is_err()
        );
    }
    let mut revoked = grant.clone();
    revoked.revoked = true;
    assert!(
        proof
            .verify(&[revoked], crate::schema::ROOT_GRAPH_IRI, "urn:fluent")
            .is_err()
    );
    let mut wrong = grant.clone();
    wrong.public_key_hex = signer(8).public_key_hex();
    assert!(
        proof
            .verify(&[wrong], crate::schema::ROOT_GRAPH_IRI, "urn:fluent")
            .is_err()
    );
    assert!(
        proof
            .verify(
                &[grant.clone(), grant],
                crate::schema::ROOT_GRAPH_IRI,
                "urn:fluent"
            )
            .is_err()
    );
}

#[test]
fn wire_cannot_supply_grants_unknown_fields_or_unbounded_evidence() {
    let (_, _, proof, _) = proof_and_grant(crate::schema::ROOT_GRAPH_IRI);
    let mut encoded = serde_json::to_value(&proof).unwrap();
    encoded["grants"] = json!([{"trusted":true}]);
    assert!(Proof::decode(&serde_json::to_vec(&encoded).unwrap()).is_err());
    encoded.as_object_mut().unwrap().remove("grants");
    encoded["body"]["auth_class"] = json!("x".repeat(MAX_FIELD + 1));
    assert!(Proof::decode(&serde_json::to_vec(&encoded).unwrap()).is_err());
    assert!(Proof::decode(&vec![b' '; MAX_PROOF + 1]).is_err());
}

#[test]
fn witness_capture_cannot_substitute_a_caller_label_for_the_native_source_store() {
    let (store, graph) = loaded(crate::schema::ROOT_GRAPH_IRI);
    let native_id = store.store_id().unwrap();
    let before = store.latest_tx_id().unwrap();
    assert!(
        capture(
            &store,
            graph,
            "urn:fluent",
            "urn:event",
            "urn:forged-source"
        )
        .is_err(),
        "caller label was accepted as source-store identity"
    );
    let captured = capture(&store, graph, "urn:fluent", "urn:event", &native_id).unwrap();
    assert_eq!(captured.0.source_store, native_id);
    let (other, other_graph) = loaded(crate::schema::ROOT_GRAPH_IRI);
    assert_ne!(other.store_id().unwrap(), native_id);
    assert!(capture(&other, other_graph, "urn:fluent", "urn:event", &native_id).is_err());
    assert_eq!(before, store.latest_tx_id().unwrap());
}
