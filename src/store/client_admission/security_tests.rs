//! Discriminators for complete candidate validation and current authority.
use super::*;
use crate::Datum;
use crate::types::Value;

fn ns(name: &str) -> String {
    format!("{}{name}", crate::namespace::DEFAULT_BASE_NS)
}

fn datum(store: &Store, subject: &str, predicate: &str, value: Value, op: Op) -> Datum {
    Datum {
        entity: store.intern(subject).unwrap(),
        attribute: store.intern(predicate).unwrap(),
        value,
        valid_from: T1.into(),
        valid_to: None,
        op,
    }
}

// Enroll/revoke only through a real amendment signed by the fixture trust root.
fn amend(f: &mut Fixture, registration: &str, mut change: Vec<Datum>, nonce: &str) {
    let digest = trust_root::digest_after(&f.store, registration, &change).unwrap();
    let signature = quipu_signing::sign_hex(
        &f.key,
        &trust_root::amendment_message(&f.store.store_id().unwrap(), registration, &digest, nonce),
    );
    let amendment = format!("urn:amendment:synthetic:{nonce}");
    change.extend([
        datum(
            &f.store,
            &amendment,
            crate::namespace::RDF_TYPE,
            Value::Ref(f.store.intern(&ns("RegistryAmendment")).unwrap()),
            Op::Assert,
        ),
        datum(
            &f.store,
            &amendment,
            &ns("amends"),
            Value::Ref(f.store.intern(registration).unwrap()),
            Op::Assert,
        ),
        datum(
            &f.store,
            &amendment,
            &ns("amendedDigest"),
            Value::Str(digest),
            Op::Assert,
        ),
        datum(
            &f.store,
            &amendment,
            &ns("amendmentSigner"),
            Value::Str("client-fixture".into()),
            Op::Assert,
        ),
        datum(
            &f.store,
            &amendment,
            &ns("amendmentNonce"),
            Value::Str(nonce.into()),
            Op::Assert,
        ),
        datum(
            &f.store,
            &amendment,
            &ns("amendmentSignature"),
            Value::Str(signature),
            Op::Assert,
        ),
    ]);
    f.store
        .transact(&change, T1, None, Some("synthetic-amendment"))
        .unwrap();
}

#[test]
fn full_candidate_validation_includes_untouched_invalid_focus_nodes() {
    let mut f = fixture_with_seed(
        "<urn:untouched> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:Item> .",
    );
    let p = proposal(&f);
    let before = state(&f.store);
    assert!(f.store.admit_client_changeset(&p).is_err());
    assert_eq!(state(&f.store), before);
    assert!(f.store.conn.is_autocommit());
}

#[test]
fn revoked_key_cannot_use_an_earlier_valid_signed_proposal() {
    let mut f = fixture();
    let p = proposal(&f);
    let registration = f.registration.clone();
    let change = vec![datum(
        &f.store,
        &registration,
        &ns("publicKey"),
        Value::Str(quipu_signing::public_key_hex(&f.key)),
        Op::Retract,
    )];
    amend(&mut f, &registration, change, &"cc".repeat(16));
    let before = state(&f.store);
    assert!(f.store.admit_client_changeset(&p).is_err());
    assert_eq!(state(&f.store), before);
    assert!(f.store.conn.is_autocommit());
}

#[test]
fn authority_cannot_be_borrowed_from_another_registration_of_the_same_signer() {
    let mut f = fixture();
    let other = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[20; 32]).unwrap();
    let registration = "urn:registration:other-client-fixture";
    let change = vec![
        datum(
            &f.store,
            registration,
            crate::namespace::RDF_TYPE,
            Value::Ref(f.store.intern(&ns("VerifierRegistration")).unwrap()),
            Op::Assert,
        ),
        datum(
            &f.store,
            registration,
            &ns("verifier"),
            Value::Str("client-fixture".into()),
            Op::Assert,
        ),
        datum(
            &f.store,
            registration,
            &ns("publicKey"),
            Value::Str(quipu_signing::public_key_hex(&other)),
            Op::Assert,
        ),
        datum(
            &f.store,
            registration,
            &ns("attests"),
            Value::Str(write_capability(GRAPH, "urn:p:blocked")),
            Op::Assert,
        ),
        datum(
            &f.store,
            registration,
            verifier_registry::TRUST_TIER,
            Value::Str(verifier_registry::AGENT_TIER.into()),
            Op::Assert,
        ),
    ];
    amend(&mut f, registration, change, &"dd".repeat(16));
    let mut values = f.overlay.read().clone();
    values.insert("<urn:item> <urn:p:blocked> \"no\"".into());
    f.overlay.replace(values).unwrap();
    let mut p = proposal(&f);
    let before = state(&f.store);
    assert!(
        f.store
            .admit_client_changeset(&p)
            .unwrap_err()
            .to_string()
            .contains("graph/predicate authority")
    );
    assert_eq!(state(&f.store), before);
    // Naming the other registration cannot make the first key its signing key.
    p.envelope.key_id = registration.into();
    resign(&f, &mut p);
    assert!(f.store.admit_client_changeset(&p).is_err());
    assert_eq!(state(&f.store), before);
}
