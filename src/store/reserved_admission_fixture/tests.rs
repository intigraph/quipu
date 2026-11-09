use super::*;
use crate::{Datum, Op, Value};
use ring::signature::{Ed25519KeyPair, KeyPair};

struct Fixture {
    store: Store,
    history: FixtureHistory,
    key: Ed25519KeyPair,
    request: Proposal,
    binding: attest::SessionBinding,
}
const NOW: u64 = 1_767_225_600;

fn datum(store: &Store, s: &str, p: &str, v: Value) -> Datum {
    Datum {
        entity: store.intern(s).unwrap(),
        attribute: store.intern(p).unwrap(),
        value: v,
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

fn fixture() -> Fixture {
    let mut store = Store::open_in_memory().unwrap();
    // The declared fixture profile is asserted/SHACL-only. Native defaults
    // enable both OWL paths; do not silently inherit a different profile.
    store.owl_config.validate_on_write = false;
    store.owl_config.reactive_materialize = false;
    let history = FixtureHistory::fresh(&store).unwrap();
    store.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
    let ns = crate::namespace::DEFAULT_BASE_NS;
    let grant = vec![
        datum(
            &store,
            "urn:fixture:principal",
            crate::namespace::RDF_TYPE,
            Value::Ref(store.intern(&format!("{ns}Principal")).unwrap()),
        ),
        datum(
            &store,
            "urn:fixture:principal",
            &format!("{ns}principalId"),
            Value::Str("fixture-agent".into()),
        ),
        datum(
            &store,
            "urn:fixture:principal",
            &format!("{ns}authorityOver"),
            Value::Str(crate::schema::ROOT_GRAPH_IRI.into()),
        ),
    ];
    store.transact(&grant, TS, None, None).unwrap();
    store.load_shapes("one", SHAPES, TS).unwrap();
    store.shacl_config.validate_on_write = true;
    store.governance_config.enforce_authority = true;
    let key = Ed25519KeyPair::from_pkcs8(
        Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
            .unwrap()
            .as_ref(),
    )
    .unwrap();
    let binding = attest::SessionBinding::new(
        "fixture-agent",
        "fixture-session",
        hex::encode(key.public_key().as_ref()),
        "fixture-introducer",
        NOW - 1,
        NOW + 600,
    )
    .unwrap();
    store.attestation_register(&binding).unwrap();
    store.attestation_set_write(&binding.session, true).unwrap();
    let touched = vec![Slot {
        subject: "<urn:e>".into(),
        predicate: "urn:p".into(),
    }];
    let coordinates = snapshot(&store, &history).unwrap();
    let mut request = Proposal {
        source: SourceBinding {
            store_id: coordinates.store_id,
            log_epoch: coordinates.log_epoch,
            graph: coordinates.graph,
            base_tx: coordinates.head_tx,
            base_slice_hash: ContentDigest([0; 32]),
            scope: Some(touched.clone()),
            inputs: vec![],
        },
        touched,
        valid_at: TS.into(),
        incoming: vec![vec!["\"first\"".into()]],
        profile: profile(&store).unwrap(),
    };
    request.source.base_slice_hash =
        slice_hash(&store, &read(&store, &history, &request).unwrap()).unwrap();
    Fixture {
        store,
        history,
        key,
        request,
        binding,
    }
}

// Deterministic labels are fixture identifiers, never production entropy.
fn nonce(label: &str) -> String {
    hex::encode(&Sha256::digest(label.as_bytes())[..16])
}

fn sign(f: &Fixture, request: &Proposal, label: &str) -> AttestationEnvelope {
    let mut envelope = AttestationEnvelope {
        version: attest::WRITE_V2.into(),
        key_id: f.binding.key_id.clone(),
        session: f.binding.session.clone(),
        introducer: f.binding.introducer.clone(),
        issued_at_epoch: NOW,
        nonce: nonce(label),
        signature: String::new(),
        audience: Some(f.store.store_id().unwrap()),
    };
    let body = attest::body_sha256(&serde_json::to_vec(request).unwrap());
    let audience = f.store.store_id().unwrap();
    envelope.signature = hex::encode(
        f.key
            .sign(&attest::canonical_message(
                &envelope,
                &payload(&body, &audience),
            ))
            .as_ref(),
    );
    envelope
}

fn attempt(f: &mut Fixture, request: &Proposal, nonce: &str) -> Result<i64> {
    let envelope = sign(f, request, nonce);
    admit(&mut f.store, true, &f.history, request, &envelope, NOW)
}
fn unchanged(f: &Fixture, head: i64, nonce: &str) {
    assert_eq!(
        f.store.latest_tx_id().unwrap(),
        head,
        "refusal changed transactions"
    );
    assert!(
        !f.store
            .attestation_nonce_spent(&f.binding.session, &self::nonce(nonce))
            .unwrap(),
        "refusal consumed nonce"
    );
    assert!(
        f.store.principal_chain().is_empty(),
        "authority scope leaked"
    );
    assert!(f.store.conn.is_autocommit(), "reservation leaked");
}

#[test]
fn disabled_and_bad_signature_never_write_or_consume() {
    let mut f = fixture();
    let request = f.request.clone();
    let head = f.store.latest_tx_id().unwrap();
    let mut envelope = sign(&f, &request, "disabled");
    let error = admit(&mut f.store, false, &f.history, &request, &envelope, NOW)
        .unwrap_err()
        .to_string();
    assert!(error.contains("disabled"), "{error}");
    unchanged(&f, head, "disabled");
    envelope.signature = "00".repeat(64);
    let error = admit(&mut f.store, true, &f.history, &request, &envelope, NOW)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("attestation signature does not verify"),
        "{error}"
    );
    unchanged(&f, head, "disabled");
}

#[test]
fn malformed_nonce_has_its_own_refusal_control() {
    let mut f = fixture();
    let request = f.request.clone();
    let head = f.store.latest_tx_id().unwrap();
    let mut envelope = sign(&f, &request, "malformed");
    envelope.nonce = "not-hex".into();
    let error = admit(&mut f.store, true, &f.history, &request, &envelope, NOW)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("nonce must be 128-bit lowercase hex"),
        "{error}"
    );
    unchanged(&f, head, "malformed");
}

#[test]
fn preparation_is_not_hash_authority_but_reserved_boundary_refuses() {
    let mut f = fixture();
    let mut request = f.request.clone();
    let head = f.store.latest_tx_id().unwrap();
    request.source.base_slice_hash = ContentDigest([99; 32]);
    assert!(
        read(&f.store, &f.history, &request).is_ok(),
        "pure reader does not verify hash"
    );
    let err = attempt(&mut f, &request, "wrong-hash")
        .unwrap_err()
        .to_string();
    assert!(err.contains("base slice hash"), "{err}");
    unchanged(&f, head, "wrong-hash");
    println!(
        "BEFORE pure reader accepted forged digest; AFTER reserved boundary refused with zero mutation"
    );
}

#[test]
fn signed_request_scope_profile_history_and_time_are_bound() {
    for fault in [
        "scope",
        "profile",
        "history",
        "time",
        "body",
        "revoked",
        "authority",
    ] {
        let mut f = fixture();
        let mut request = f.request.clone();
        let head = f.store.latest_tx_id().unwrap();
        let envelope = sign(&f, &request, fault);
        match fault {
            "scope" => request.source.scope = Some(vec![]),
            "profile" => request.profile = ContentDigest([1; 32]),
            "history" => f.history.epoch = "different".into(),
            "time" => request.valid_at = "2026-01-01".into(),
            "body" => request.incoming[0][0] = "\"changed\"".into(),
            "revoked" => f.store.attestation_revoke(&f.binding.session).unwrap(),
            "authority" => f.store.governance_config.enforce_authority = false,
            _ => unreachable!(),
        }
        // Re-sign structural faults so those controls reach the actual guard.
        let envelope = if matches!(fault, "scope" | "profile" | "time") {
            sign(&f, &request, fault)
        } else {
            envelope
        };
        let expected = match fault {
            "scope" => "declared/request scope mismatch",
            "profile" => "profile binding",
            "history" => "history witness identity",
            "time" => "explicit canonical UTC valid time required",
            "body" => "attestation signature does not verify",
            "revoked" => "revoked attestation session",
            "authority" => "authority enforcement required",
            _ => unreachable!(),
        };
        let error = admit(&mut f.store, true, &f.history, &request, &envelope, NOW)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(expected),
            "{fault}: expected {expected}, got {error}"
        );
        unchanged(&f, head, fault);
    }
}

#[cfg(feature = "shacl")]
#[test]
fn native_signed_admission_commits_and_replay_refuses() {
    let mut f = fixture();
    let request = f.request.clone();
    let envelope = sign(&f, &request, "native-ok");
    let tx = admit(&mut f.store, true, &f.history, &request, &envelope, NOW).unwrap();
    assert_eq!(f.store.latest_tx_id().unwrap(), tx);
    assert!(
        f.store
            .attestation_nonce_spent(&f.binding.session, &envelope.nonce)
            .unwrap()
    );
    assert!(f.store.principal_chain().is_empty());
    let error = admit(&mut f.store, true, &f.history, &request, &envelope, NOW)
        .unwrap_err()
        .to_string();
    assert!(error.contains("attestation nonce replay"), "{error}");
    assert_eq!(f.store.latest_tx_id().unwrap(), tx);
    let values = f
        .store
        .entity_facts(f.store.lookup("urn:e").unwrap().unwrap())
        .unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].value, Value::Str("first".into()));
}

#[cfg(not(feature = "shacl"))]
#[test]
fn absent_validator_refuses_without_partial_write() {
    let mut f = fixture();
    let request = f.request.clone();
    let head = f.store.latest_tx_id().unwrap();
    let error = attempt(&mut f, &request, "no-shacl")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("write SHACL requires the shacl feature"),
        "{error}"
    );
    unchanged(&f, head, "no-shacl");
}

#[cfg(feature = "shacl")]
#[test]
fn current_divergence_is_not_last_writer_wins() {
    let mut f = fixture();
    let request = f.request.clone();
    attempt(&mut f, &request, "first").unwrap();
    let mut stale = request;
    stale.incoming[0][0] = "\"competing\"".into();
    let head = f.store.latest_tx_id().unwrap();
    let error = attempt(&mut f, &stale, "stale").unwrap_err().to_string();
    assert!(error.contains("unresolved conflict"), "{error}");
    unchanged(&f, head, "stale");
}

#[cfg(feature = "shacl")]
#[test]
fn full_validation_refuses_old_debt_outside_incremental_focus() {
    let mut f = fixture();
    f.store.shacl_config.validate_on_write = false;
    f.store.governance_config.enforce_authority = false;
    let debt = vec![
        datum(&f.store, "urn:old", "urn:p", Value::Str("a".into())),
        datum(&f.store, "urn:old", "urn:p", Value::Str("b".into())),
    ];
    f.store.transact(&debt, TS, None, None).unwrap();
    f.store.shacl_config.validate_on_write = true;
    let unrelated = datum(&f.store, "urn:probe", "urn:p", Value::Str("ok".into()));
    // Native incremental acceptance is deliberately not full conformance.
    let baseline = f.store.speculate(&[unrelated], TS, |_| Ok(()));
    assert!(
        baseline.is_ok(),
        "baseline incremental gate must accept unrelated focus"
    );
    f.store.governance_config.enforce_authority = true;
    let request = f.request.clone();
    let head = f.store.latest_tx_id().unwrap();
    let error = attempt(&mut f, &request, "old-debt")
        .unwrap_err()
        .to_string();
    assert!(error.contains("full staged graph violates"), "{error}");
    unchanged(&f, head, "old-debt");
}
