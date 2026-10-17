use quipu::reasoner::history_attestation::*;
use quipu::signing::{public_key_hex, sign_hex};
use ring::{rand::SystemRandom, signature::Ed25519KeyPair};
use sha2::{Digest, Sha256};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct Fixture {
    key: Ed25519KeyPair,
    public_key: String,
    envelope: SignedHistoryAttestation,
}

impl Fixture {
    fn new() -> Self {
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap();
        let public_key = public_key_hex(&key);
        let payload = HistoryAttestation {
            schema_version: 1,
            issuer_id: "synthetic-reviewer".into(),
            grant_policy_sha256: hash(b"private test policy v1"),
            reviewed_evidence_ref: "urn:test:coverage-review:1".into(),
            reviewed_evidence_sha256: hash(b"includes pre-window catalogue and premise history"),
            raw_artifact_sha256: hash(b"synthetic retained artifact"),
            artifact_kind: HistoryArtifactKind::RawSqliteSnapshot,
            provider_scope: HistoryProviderScope::NativeOnly,
            coverage: HistoryCoverage {
                source_store_id: "synthetic-lineage".into(),
                catalogue_graph: "urn:test:catalogue".into(),
                premise_graph: "urn:test:premises".into(),
                transaction_first: 1,
                transaction_last: 9,
                valid_time_from: 100,
                valid_time_until: 200,
            },
            issued_at: 300,
            expires_at: 400,
        };
        let signature = sign_hex(&key, &history_attestation_message(&payload).unwrap());
        Self {
            key,
            public_key,
            envelope: SignedHistoryAttestation { payload, signature },
        }
    }

    fn grant(&self) -> HistoryCoverageGrant<'_> {
        HistoryCoverageGrant {
            issuer_id: "synthetic-reviewer",
            public_key_hex: &self.public_key,
            policy_sha256: &self.envelope.payload.grant_policy_sha256,
            policy_bytes: b"private test policy v1",
            reviewed_evidence_ref: "urn:test:coverage-review:1",
            reviewed_evidence_sha256: &self.envelope.payload.reviewed_evidence_sha256,
            coverage: &self.envelope.payload.coverage,
        }
    }
}

const EVIDENCE: &[u8] = b"includes pre-window catalogue and premise history";

#[test]
fn signed_terms_bind_external_policy_and_actual_review_bytes() {
    let fixture = Fixture::new();
    let grant = fixture.grant();
    let receipt =
        verify_history_attestation(&fixture.envelope, &grant, Some(EVIDENCE), 350).unwrap();
    assert_eq!(receipt.payload(), &fixture.envelope.payload);
    assert_eq!(receipt.admitted_at(), 350);
    assert!(matches!(
        verify_history_attestation(&fixture.envelope, &grant, None, 350),
        Err(HistoryAttestationError::EvidenceUnavailable)
    ));
    assert!(matches!(
        verify_history_attestation(&fixture.envelope, &grant, Some(b"changed review"), 350),
        Err(HistoryAttestationError::EvidenceMismatch)
    ));
    let wrong_policy = HistoryCoverageGrant {
        policy_bytes: b"changed policy",
        ..fixture.grant()
    };
    assert!(matches!(
        verify_history_attestation(&fixture.envelope, &wrong_policy, Some(EVIDENCE), 350),
        Err(HistoryAttestationError::EvidenceMismatch)
    ));
    // The external grant cannot be widened by even a correctly signed issuer.
    let mut broader = fixture.envelope.clone();
    broader.payload.coverage.transaction_last = 10;
    broader.signature = sign_hex(
        &fixture.key,
        &history_attestation_message(&broader.payload).unwrap(),
    );
    assert!(matches!(
        verify_history_attestation(&broader, &grant, Some(EVIDENCE), 350),
        Err(HistoryAttestationError::Unauthorized)
    ));
    let mut changed = fixture.envelope.clone();
    changed.payload.raw_artifact_sha256 = hash(b"replacement artifact");
    assert!(matches!(
        verify_history_attestation(&changed, &grant, Some(EVIDENCE), 350),
        Err(HistoryAttestationError::Signature)
    ));
}

#[test]
fn fresh_admission_expiry_does_not_mutate_a_retained_receipt() {
    let fixture = Fixture::new();
    let grant = fixture.grant();
    let original =
        verify_history_attestation(&fixture.envelope, &grant, Some(EVIDENCE), 300).unwrap();
    for time in [299, 400, 401] {
        assert!(matches!(
            verify_history_attestation(&fixture.envelope, &grant, Some(EVIDENCE), time),
            Err(HistoryAttestationError::NotCurrent)
        ));
    }
    assert_eq!(original.admitted_at(), 300);
    assert_eq!(original.payload().expires_at, 400);
}

#[test]
fn strict_versioned_serialization_refuses_ambiguous_or_extra_fields() {
    let fixture = Fixture::new();
    let bytes = serde_json::to_vec(&fixture.envelope).unwrap();
    let parsed: SignedHistoryAttestation = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        history_attestation_message(&parsed.payload).unwrap(),
        history_attestation_message(&fixture.envelope.payload).unwrap()
    );
    assert!(
        history_attestation_message(&parsed.payload)
            .unwrap()
            .starts_with(b"quipu-history-source/v1\0{\"schema_version\":1,\"issuer_id\":")
    );
    let mut value = serde_json::to_value(&fixture.envelope).unwrap();
    value["payload"]["coverage"]["complete"] = true.into();
    assert!(serde_json::from_value::<SignedHistoryAttestation>(value).is_err());
    let duplicate = String::from_utf8(bytes).unwrap().replace(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
    );
    assert!(serde_json::from_str::<SignedHistoryAttestation>(&duplicate).is_err());
    let mut provider = serde_json::to_value(&fixture.envelope).unwrap();
    provider["payload"]["provider_scope"] = "attached".into();
    assert!(serde_json::from_value::<SignedHistoryAttestation>(provider).is_err());
}
