//! Portable signature and overlay controls; these also compile for Wasm.
use super::*;

fn key() -> ring::signature::Ed25519KeyPair {
    ring::signature::Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap()
}

fn overlay() -> ClientOverlay {
    let base = BaseJournal::new(
        "urn:store:test".into(),
        "urn:epoch:test".into(),
        3,
        "ROOT".into(),
        digest(b"shapes"),
        [BaseRow {
            triple: "<urn:s> <urn:p> \"base\"".into(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
        }],
    )
    .unwrap();
    let mut overlay = ClientOverlay::open(base, "urn:fork:synthetic-client".into()).unwrap();
    overlay
        .replace(["<urn:s> <urn:p> \"local\"".into()])
        .unwrap();
    overlay
}

fn identity() -> ProposalIdentity {
    ProposalIdentity {
        key_id: "urn:reg:client".into(),
        signer: "fixture".into(),
        operation_id: "00000000-0000-4000-8000-000000000001".into(),
        issued_at: "2026-01-02T00:00:00Z".into(),
        nonce: "11".repeat(16),
    }
}

#[test]
fn portable_shared_signer_verifies_exact_envelope_and_domain() {
    let signed = overlay().sign(&key(), identity()).unwrap();
    let pk = quipu_signing::public_key_hex(&key());
    signed.verify(&pk).unwrap();
    let message = signed.envelope.message().unwrap();
    assert!(message.starts_with(b"quipu-changeset-v1\nrole=client\n"));
    assert!(!quipu_signing::verify_hex(
        &pk,
        b"v1|not-this-domain",
        &signed.signature
    ));
    let cs = signed.changeset().unwrap();
    assert_eq!(
        signed.envelope.delta_sha256,
        format!("sha256:{}", cs.digest())
    );
    assert_eq!(cs.deletes[0].triple, "<urn:s> <urn:p> \"base\"");
    assert_eq!(cs.inserts[0].triple, "<urn:s> <urn:p> \"local\"");
}

#[test]
fn every_signed_field_refuses_tampering() {
    let original = overlay().sign(&key(), identity()).unwrap();
    let object = serde_json::to_value(&original.envelope).unwrap();
    let pk = quipu_signing::public_key_hex(&key());
    for field in object.as_object().unwrap().keys() {
        let mut value = object.clone();
        if field == "base_tx" {
            value[field] = serde_json::json!(4);
        } else {
            value[field] = serde_json::json!(format!("{}x", value[field].as_str().unwrap()));
        }
        let mut changed = original.clone();
        changed.envelope = serde_json::from_value(value).unwrap();
        assert!(changed.verify(&pk).is_err(), "unbound field {field}");
    }
}

#[test]
fn unsigned_headers_are_bound_even_after_body_and_delta_rehashing() {
    let original = overlay().sign(&key(), identity()).unwrap();
    let mut changed = original.clone();
    changed.body = changed.body.replace("client-overlay", "another-source");
    changed.envelope.body_sha256 = digest(changed.body.as_bytes());
    assert_eq!(
        changed.envelope.delta_sha256,
        original.envelope.delta_sha256
    );
    assert!(
        changed
            .verify(&quipu_signing::public_key_hex(&key()))
            .is_err()
    );
    assert_ne!(
        changed.fingerprint().unwrap(),
        original.fingerprint().unwrap()
    );
}

#[test]
fn private_overlay_retains_frozen_base_across_serialization_and_failed_edit() {
    let mut local = overlay();
    let before = local.base().clone();
    let proposed = local.read().clone();
    assert!(local.replace(["_:blank <urn:p> \"bad\"".into()]).is_err());
    assert_eq!(local.read(), &proposed);
    assert_eq!(local.base(), &before);
    let restored: ClientOverlay =
        serde_json::from_slice(&serde_json::to_vec(&local).unwrap()).unwrap();
    assert_eq!(restored.base(), &before);
    assert_eq!(restored.changeset().unwrap(), local.changeset().unwrap());
    assert_eq!(
        restored.base().slice_hash().unwrap(),
        before.slice_hash().unwrap()
    );
}

#[test]
fn interval_and_graph_are_part_of_the_base_digest() {
    let original = overlay().base().clone();
    let row = original.rows().first().unwrap();
    let mut later = row.clone();
    later.valid_from = "2026-01-01T01:00:00Z".into();
    let changed = BaseJournal::new(
        original.store_id.clone(),
        original.log_epoch.clone(),
        3,
        original.graph.clone(),
        original.shapes_hash.clone(),
        [later],
    )
    .unwrap();
    assert_ne!(
        original.slice_hash().unwrap(),
        changed.slice_hash().unwrap()
    );
    let graph = BaseJournal::new(
        original.store_id.clone(),
        original.log_epoch.clone(),
        3,
        "urn:other:graph".into(),
        original.shapes_hash.clone(),
        [row.clone()],
    )
    .unwrap();
    assert_ne!(original.slice_hash().unwrap(), graph.slice_hash().unwrap());
}

#[test]
fn invalid_envelope_newline_and_non_uuid_operation_refuse_before_signing() {
    let mut id = identity();
    id.signer = "fixture\nrole=master".into();
    assert!(overlay().sign(&key(), id).is_err());
    let mut id = identity();
    id.operation_id = "not-a-uuid".into();
    assert!(overlay().sign(&key(), id).is_err());
    let mut id = identity();
    id.nonce = "AA".repeat(16);
    assert!(overlay().sign(&key(), id).is_err());
}

#[test]
fn scope_context_and_list_order_have_the_existing_cut_semantics() {
    let graphs = vec!["urn:b".into(), "urn:a".into()];
    let reverse = vec!["urn:a".into(), "urn:b".into()];
    // Existing sorted-key JSON wire vector, independent of both entry points.
    assert_eq!(
        crate::replica_manifest::scope_digest(&graphs, &[]),
        "sha256:c6b0690ab3c0f52e912525e537c194a6a9f782238ba6f40d98ae3b589d6b36de"
    );
    assert_eq!(
        crate::replica_manifest::scope_digest(&graphs, &[]),
        crate::replica_manifest::scope_digest(&reverse, &[])
    );
    assert_ne!(
        crate::replica_manifest::scope_digest(&graphs, &[]),
        crate::replica_manifest::scope_digest(&[], &graphs)
    );
    #[cfg(not(target_arch = "wasm32"))]
    assert_eq!(
        crate::replica_manifest::scope_digest(&graphs, &[]),
        crate::asof_share::scope_digest(&graphs, &[])
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[path = "client_changeset_property_tests.rs"]
mod property_tests;
