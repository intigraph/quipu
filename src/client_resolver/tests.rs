use super::*;
use crate::client_changeset::{BaseJournal, BaseRow, ClientOverlay, ProposalIdentity};
use crate::resolver_contract::Slot;

fn key(seed: u8) -> ring::signature::Ed25519KeyPair {
    ring::signature::Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
}

fn fixture() -> (SignedChangeSet, SignedResolverArtifact) {
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../test-fixtures/resolver-contract-v1.json"
    ))
    .unwrap();
    let manifest: ValidatorManifest =
        serde_json::from_value(vectors["vectors"][0]["manifest"].clone()).unwrap();
    let mut candidate: ProspectiveCandidate =
        serde_json::from_value(vectors["vectors"][0]["candidate"].clone()).unwrap();
    let journal = BaseJournal::new(
        "urn:store:resolver-fixture".into(),
        "urn:epoch:resolver-fixture".into(),
        3,
        "ROOT".into(),
        digest(&manifest.combined_shapes),
        [BaseRow {
            triple: "<urn:s> <urn:p> \"base\"".into(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
        }],
    )
    .unwrap();
    let mut overlay = ClientOverlay::open(journal, "urn:fork:resolver-fixture".into()).unwrap();
    overlay
        .replace(["<urn:s> <urn:p> \"local\"".into()])
        .unwrap();
    let proposal = overlay
        .sign(
            &key(7),
            ProposalIdentity {
                key_id: "urn:registration:resolver-fixture".into(),
                signer: "resolver-fixture".into(),
                operation_id: "00000000-0000-4000-8000-000000000001".into(),
                issued_at: "2026-01-02T00:00:00Z".into(),
                nonce: "11".repeat(16),
            },
        )
        .unwrap();
    candidate.source.store_id = proposal.envelope.store_id.clone();
    candidate.source.log_epoch = proposal.envelope.log_epoch.clone();
    candidate.source.graph = proposal.envelope.graph.clone();
    candidate.source.base_tx = u64::try_from(proposal.envelope.base_tx).unwrap();
    candidate.source.base_slice_hash = digest_value(&proposal.envelope.base_slice_hash).unwrap();
    candidate.source.scope = None;
    candidate.validator = manifest.identity().unwrap();
    let artifact = ResolverArtifact::new(&proposal, candidate, manifest).unwrap();
    let signature = quipu_signing::sign_hex(&key(7), &artifact.message(&proposal).unwrap());
    (
        proposal,
        SignedResolverArtifact {
            artifact,
            signature,
        },
    )
}

#[test]
fn separate_shared_crate_signature_preserves_all_v1_bytes() {
    let (proposal, artifact) = fixture();
    let original = serde_json::to_vec(&proposal).unwrap();
    let v1_message = proposal.envelope.message().unwrap();
    let v1_fingerprint = proposal.fingerprint().unwrap();
    let public_key = quipu_signing::public_key_hex(&key(7));
    artifact.verify(&proposal, &public_key).unwrap();
    assert!(v1_message.starts_with(b"quipu-changeset-v1\nrole=client\n"));
    assert!(
        artifact
            .artifact
            .message(&proposal)
            .unwrap()
            .starts_with(b"quipu-client-resolver-artifact-v1\0")
    );
    assert_eq!(serde_json::to_vec(&proposal).unwrap(), original);
    assert_eq!(proposal.envelope.message().unwrap(), v1_message);
    assert_eq!(proposal.fingerprint().unwrap(), v1_fingerprint);
    let wire = serde_json::to_string_pretty(&artifact).unwrap();
    let roundtrip: SignedResolverArtifact = serde_json::from_str(&wire).unwrap();
    assert_eq!(roundtrip, artifact);
    roundtrip.verify(&proposal, &public_key).unwrap();
}

#[test]
fn both_domains_and_same_registered_key_are_required() {
    let (proposal, original) = fixture();
    let public_key = quipu_signing::public_key_hex(&key(7));
    assert!(original.artifact.clone().sign(&proposal, &key(8)).is_err());
    let signed = original.artifact.clone().sign(&proposal, &key(7)).unwrap();
    assert_eq!(signed, original);
    let mut changed = original.clone();
    changed.signature = quipu_signing::sign_hex(&key(7), &proposal.envelope.message().unwrap());
    assert!(changed.verify(&proposal, &public_key).is_err());
    changed.signature =
        quipu_signing::sign_hex(&key(8), &original.artifact.message(&proposal).unwrap());
    assert!(changed.verify(&proposal, &public_key).is_err());
    assert!(
        original
            .verify(&proposal, &quipu_signing::public_key_hex(&key(8)))
            .is_err()
    );
}

#[test]
fn every_source_input_and_its_order_are_signed() {
    let (proposal, original) = fixture();
    let public_key = quipu_signing::public_key_hex(&key(7));
    for index in 0..original.artifact.candidate.source.inputs.len() {
        let mut changed = original.clone();
        changed.artifact.candidate.source.inputs[index]
            .content
            .push(b'x');
        assert!(changed.verify(&proposal, &public_key).is_err());
        let mut changed = original.clone();
        changed.artifact.candidate.source.inputs[index]
            .version
            .push('x');
        assert!(changed.verify(&proposal, &public_key).is_err());
    }
    let mut changed = original.clone();
    changed.artifact.candidate.source.inputs.swap(0, 1);
    assert!(changed.verify(&proposal, &public_key).is_err());
}

#[test]
fn decisions_and_rehashed_validator_mutations_cannot_reuse_the_signature() {
    let (proposal, original) = fixture();
    let public_key = quipu_signing::public_key_hex(&key(7));
    let mut changed = original.clone();
    changed.artifact.candidate.decisions.clear();
    assert!(changed.verify(&proposal, &public_key).is_err());
    let mut changed = original.clone();
    changed.artifact.manifest.flags.recording_verdicts =
        !changed.artifact.manifest.flags.recording_verdicts;
    changed.artifact.candidate.validator = changed.artifact.manifest.identity().unwrap();
    // A caller can recompute an unsigned digest, but cannot update a signature.
    changed.artifact.check_proposal(&proposal).unwrap();
    assert!(changed.verify(&proposal, &public_key).is_err());
}

#[test]
fn wrong_source_version_partial_and_empty_scope_refuse_before_signing() {
    let (proposal, original) = fixture();
    for scope in [
        Some(Vec::new()),
        Some(vec![Slot {
            subject: "<urn:s>".into(),
            predicate: "<urn:p>".into(),
        }]),
    ] {
        let mut changed = original.artifact.clone();
        changed.candidate.source.scope = scope;
        assert!(changed.message(&proposal).is_err());
    }
    let mut changed = original.artifact.clone();
    changed.version = 2;
    assert!(changed.message(&proposal).is_err());
    let mut changed = original.artifact.clone();
    changed.candidate.source.log_epoch.push('x');
    assert!(changed.message(&proposal).is_err());
    let mut changed = original.artifact;
    changed.candidate.source.base_slice_hash.0[0] ^= 1;
    assert!(changed.message(&proposal).is_err());
}

#[test]
fn unknown_wire_fields_are_not_ignored() {
    let (_, artifact) = fixture();
    let mut wire = serde_json::to_value(artifact).unwrap();
    wire["authority"] = serde_json::json!("pretend-admitted");
    assert!(serde_json::from_value::<SignedResolverArtifact>(wire).is_err());
}

#[cfg(all(feature = "shacl", target_os = "linux"))]
#[path = "observation_tests.rs"]
mod observation_tests;
