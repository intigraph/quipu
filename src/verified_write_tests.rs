//! The canonical signed-request check and the opaque verified write
//! (aegis-2idcev slice 1). Synthetic keys and in-memory stores only.
use ring::signature::{Ed25519KeyPair, KeyPair as _};

use super::*;
use crate::session_attestation::{SessionBinding, WRITE_V1, canonical_message};

const BODY: &[u8] = br#"{"triples":"<urn:x> <urn:p> \"1\" ."}"#;

fn store_with_session() -> (Store, Ed25519KeyPair, SessionBinding) {
    let store = Store::open_in_memory().unwrap();
    let key = Ed25519KeyPair::from_seed_unchecked(&[61; 32]).unwrap();
    let now = crate::time::epoch_secs();
    let mut binding = SessionBinding::new(
        "synthetic-agent",
        "synthetic-session",
        hex::encode(key.public_key().as_ref()),
        "synthetic-introducer",
        now - 10,
        now + 3600,
    )
    .unwrap();
    binding.allow_write = true;
    store.attestation_register(&binding).unwrap();
    (store, key, binding)
}

/// Sign `request` for `audience` (v2) or for no audience (v1).
fn sign(
    key: &Ed25519KeyPair,
    binding: &SessionBinding,
    nonce: &str,
    request: SignedRequest<'_>,
    audience: Option<&str>,
) -> AttestationEnvelope {
    let mut envelope = AttestationEnvelope {
        version: if audience.is_some() {
            WRITE_V2
        } else {
            WRITE_V1
        }
        .into(),
        key_id: binding.key_id.clone(),
        session: binding.session.clone(),
        introducer: binding.introducer.clone(),
        issued_at_epoch: crate::time::epoch_secs(),
        nonce: nonce.into(),
        signature: String::new(),
        audience: audience.map(str::to_owned),
    };
    let hash = body_sha256(request.body);
    let message = canonical_message(
        &envelope,
        &SignedBinding::Write(WriteBinding {
            method: request.method,
            path: request.path,
            content_type: request.content_type,
            body_sha256: &hash,
            audience,
        }),
    );
    envelope.signature = hex::encode(key.sign(&message).as_ref());
    envelope
}

fn request(body: &[u8]) -> SignedRequest<'_> {
    SignedRequest {
        method: "POST",
        path: "/knot",
        content_type: "application/turtle",
        body,
    }
}

fn now() -> u64 {
    crate::time::epoch_secs()
}

fn verdict<T: std::fmt::Debug>(result: Result<T, Refusal>) -> &'static str {
    result.unwrap_err().verdict
}

#[test]
fn a_v2_signed_write_verifies_with_its_exact_bytes_and_audience() {
    let (store, key, binding) = store_with_session();
    let id = store.store_id().unwrap();
    let envelope = sign(
        &key,
        &binding,
        "00000000000000000000000000000005",
        request(BODY),
        Some(&id),
    );
    let verified = verify_signed_write(&store, envelope, request(BODY), now()).unwrap();
    assert_eq!(verified.agent(), "synthetic-agent");
    assert_eq!(verified.session(), "synthetic-session");
    assert_eq!(verified.introducer(), "synthetic-introducer");
    assert_eq!(verified.key_id(), binding.key_id);
    assert_eq!(verified.audience(), id);
    assert_eq!(verified.body(), BODY);
    assert_eq!(verified.body_sha256(), body_sha256(BODY));
    assert_eq!((verified.method(), verified.path()), ("POST", "/knot"));
    assert_eq!(verified.content_type(), "application/turtle");
    assert_eq!(verified.nonce(), "00000000000000000000000000000005");
    // Verification spends nothing: the writer barrier does that.
    assert!(
        !store
            .attestation_nonce_spent("synthetic-session", "00000000000000000000000000000005")
            .unwrap()
    );
}

#[test]
fn the_same_signature_over_other_bytes_is_refused() {
    let (store, key, binding) = store_with_session();
    let id = store.store_id().unwrap();
    let signed = sign(
        &key,
        &binding,
        "00000000000000000000000000000001",
        request(BODY),
        Some(&id),
    );
    let other = br#"{"triples":"<urn:x> <urn:p> \"2\" ."}"#;
    let cases = [
        SignedRequest {
            body: other,
            ..request(BODY)
        },
        SignedRequest {
            method: "PUT",
            ..request(BODY)
        },
        SignedRequest {
            path: "/update",
            ..request(BODY)
        },
        SignedRequest {
            content_type: "application/n-triples",
            ..request(BODY)
        },
    ];
    for changed in cases {
        assert_eq!(
            verdict(verify_signed_write(&store, signed.clone(), changed, now())),
            "badsig",
            "{changed:?}"
        );
    }
}

#[test]
fn a_signature_for_another_store_is_refused() {
    let (store, key, binding) = store_with_session();
    let (other, _, _) = store_with_session();
    other.attestation_register(&binding).ok();
    let elsewhere = other.store_id().unwrap();
    assert_ne!(elsewhere, store.store_id().unwrap(), "control: two stores");
    let envelope = sign(
        &key,
        &binding,
        "00000000000000000000000000000002",
        request(BODY),
        Some(&elsewhere),
    );
    let refusal = verify_signed_write(&store, envelope.clone(), request(BODY), now()).unwrap_err();
    assert!(
        refusal.message.contains("audience mismatch"),
        "{}",
        refusal.message
    );
    // Editing the audience claim to this store does not help: the signature
    // still covers the other store's id.
    let mut relabelled = envelope.clone();
    relabelled.audience = Some(store.store_id().unwrap());
    assert_eq!(
        verdict(verify_signed_write(
            &store,
            relabelled,
            request(BODY),
            now()
        )),
        "badsig"
    );
    // Control: the store it was signed for accepts it.
    verify_signed_write(&other, envelope, request(BODY), now()).unwrap();
}

#[test]
fn a_v1_envelope_is_not_downgraded_to_a_verified_write() {
    let (store, key, binding) = store_with_session();
    let v1 = sign(
        &key,
        &binding,
        "0000000000000000000000000000000b",
        request(BODY),
        None,
    );
    let refusal = verify_signed_write(&store, v1.clone(), request(BODY), now()).unwrap_err();
    assert_eq!(refusal.verdict, "invalid");
    assert!(refusal.message.contains("write-v2"), "{}", refusal.message);
    // Control: the shared core still accepts v1 for the HTTP adapter.
    let (_, audience) = check_signed_request(&store, &v1, request(BODY), now()).unwrap();
    assert!(audience.is_none());
}

#[test]
fn an_unaudited_route_and_an_oversize_body_are_refused() {
    let (store, key, binding) = store_with_session();
    let id = store.store_id().unwrap();
    let route = SignedRequest {
        path: "/set",
        ..request(BODY)
    };
    let envelope = sign(
        &key,
        &binding,
        "00000000000000000000000000000006",
        route,
        Some(&id),
    );
    let refusal = verify_signed_write(&store, envelope, route, now()).unwrap_err();
    assert!(
        refusal.message.contains("accepted on"),
        "{}",
        refusal.message
    );
    let big = vec![b' '; MAX_SIGNED_BODY + 1];
    let envelope = sign(
        &key,
        &binding,
        "00000000000000000000000000000003",
        request(&big),
        Some(&id),
    );
    let refusal = verify_signed_write(&store, envelope, request(&big), now()).unwrap_err();
    assert!(
        refusal.message.contains("size limit"),
        "{}",
        refusal.message
    );
}

#[test]
fn a_spent_nonce_a_revoked_session_and_a_forged_envelope_are_refused() {
    let (store, key, binding) = store_with_session();
    let id = store.store_id().unwrap();
    let spent = sign(
        &key,
        &binding,
        "00000000000000000000000000000009",
        request(BODY),
        Some(&id),
    );
    assert!(
        store
            .attestation_ensure_spent("synthetic-session", "00000000000000000000000000000009")
            .unwrap()
    );
    assert_eq!(
        verdict(verify_signed_write(&store, spent, request(BODY), now())),
        "replay"
    );
    // Forged: a valid-looking envelope signed by another key.
    let other = Ed25519KeyPair::from_seed_unchecked(&[62; 32]).unwrap();
    let forged = sign(
        &other,
        &binding,
        "00000000000000000000000000000004",
        request(BODY),
        Some(&id),
    );
    assert_eq!(
        verdict(verify_signed_write(&store, forged, request(BODY), now())),
        "badsig"
    );
    // Unsigned: an empty signature.
    let mut unsigned = sign(
        &key,
        &binding,
        "0000000000000000000000000000000a",
        request(BODY),
        Some(&id),
    );
    unsigned.signature.clear();
    assert_ne!(
        verdict(verify_signed_write(&store, unsigned, request(BODY), now())),
        ""
    );
    // Unregistered session.
    let mut stranger = sign(
        &key,
        &binding,
        "00000000000000000000000000000008",
        request(BODY),
        Some(&id),
    );
    stranger.session = "nobody".into();
    assert!(verify_signed_write(&store, stranger, request(BODY), now()).is_err());
    // Revoked after registration.
    let fresh = sign(
        &key,
        &binding,
        "00000000000000000000000000000007",
        request(BODY),
        Some(&id),
    );
    store.attestation_revoke("synthetic-session").unwrap();
    assert_eq!(
        verdict(verify_signed_write(&store, fresh, request(BODY), now())),
        "revoked"
    );
}
