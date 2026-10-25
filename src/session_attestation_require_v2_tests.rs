//! `require_v2`: a PER-BINDING refusal of audience-free v1 writes
//! (aegis-2idcev (C')2, wu ruling [81]). The pair the ruling asks for is the
//! point of this file: an unenrolled v1 signer is still accepted, and the same
//! key enrolled is refused on v1. A global refusal would refuse every sd write,
//! because sd signs only quipu-write-v1.

use ring::signature::{Ed25519KeyPair, KeyPair};

use super::{
    ATTESTATION_SKEW_SECS, AttestationEnvelope, BindingRegistry, SessionBinding, SignedBinding,
    WRITE_V1, WRITE_V2, WriteBinding, canonical_message, check_binding_deferred,
};
use crate::Store;

const AUDIENCE: &str = "urn:quipu:store:require-v2-test";
const BODY_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn key(seed: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
}

fn writer(key: &Ed25519KeyPair, session: &str, now: u64) -> SessionBinding {
    let mut binding = SessionBinding::new(
        "urn:crew:signer",
        session,
        hex::encode(key.public_key().as_ref()),
        "urn:crew:lead",
        now - 60,
        now + 3600,
    )
    .unwrap();
    binding.allow_write = true;
    binding
}

fn write(audience: Option<&str>) -> WriteBinding<'_> {
    WriteBinding {
        method: "POST",
        path: "/knot",
        content_type: "application/json",
        body_sha256: BODY_HASH,
        audience,
    }
}

/// `(envelope, payload)` for one signed write, under v1 or v2.
fn signed<'a>(
    key: &Ed25519KeyPair,
    binding: &SessionBinding,
    v2: bool,
    now: u64,
) -> (AttestationEnvelope, SignedBinding<'a>) {
    let audience = v2.then_some(AUDIENCE);
    let payload = SignedBinding::Write(write(audience));
    let mut envelope = AttestationEnvelope {
        version: if v2 { WRITE_V2 } else { WRITE_V1 }.into(),
        key_id: binding.key_id.clone(),
        session: binding.session.clone(),
        introducer: binding.introducer.clone(),
        issued_at_epoch: now,
        nonce: "0123456789abcdef0123456789abcdef".into(),
        signature: String::new(),
        audience: audience.map(str::to_owned),
    };
    envelope.signature = hex::encode(key.sign(&canonical_message(&envelope, &payload)).as_ref());
    (envelope, payload)
}

fn verdict(
    registry: &BindingRegistry,
    (envelope, payload): &(AttestationEnvelope, SignedBinding<'_>),
    now: u64,
) -> Result<(), &'static str> {
    check_binding_deferred(registry, envelope, payload, now, ATTESTATION_SKEW_SECS)
        .map(|_| ())
        .map_err(|r| r.verdict)
}

#[test]
fn require_v2_refuses_only_the_enrolled_binding_on_v1() {
    let now = crate::time::epoch_secs();
    let (k_open, k_enrolled) = (key(61), key(62));
    let open = writer(&k_open, "unenrolled", now);
    let mut enrolled = writer(&k_enrolled, "enrolled", now);
    enrolled.require_v2 = true;
    let registry = BindingRegistry::default();
    registry.register(open.clone()).unwrap();
    registry.register(enrolled.clone()).unwrap();

    // The pair: same store, same registry, one flag apart.
    assert_eq!(
        verdict(&registry, &signed(&k_open, &open, false, now), now),
        Ok(()),
        "an unenrolled v1 signer must still be accepted: the refusal is never global"
    );
    let (envelope, payload) = signed(&k_enrolled, &enrolled, false, now);
    let refusal =
        check_binding_deferred(&registry, &envelope, &payload, now, ATTESTATION_SKEW_SECS)
            .unwrap_err();
    assert_eq!(refusal.verdict, "invalid");
    assert!(refusal.message.contains(WRITE_V2), "{}", refusal.message);

    // Positive controls: the enrolled binding signs v2 fine, and so may the
    // unenrolled one, so the refusal above is the version and nothing else.
    assert_eq!(
        verdict(&registry, &signed(&k_enrolled, &enrolled, true, now), now),
        Ok(())
    );
    assert_eq!(
        verdict(&registry, &signed(&k_open, &open, true, now), now),
        Ok(())
    );
}

#[test]
fn the_flag_never_travels_in_a_serialized_binding() {
    // A producer's binding rides in a share manifest. A flag carried there
    // could tighten (or, read the other way, loosen) a consumer's policy, so it
    // is neither emitted nor accepted, exactly like allow_write.
    let now = crate::time::epoch_secs();
    let mut b = writer(&key(63), "serde", now);
    b.require_v2 = true;
    let json = serde_json::to_value(&b).unwrap();
    assert!(json.get("require_v2").is_none(), "{json}");
    let mut forged = json;
    forged["require_v2"] = serde_json::Value::Bool(true);
    let parsed: SessionBinding = serde_json::from_value(forged).unwrap();
    assert!(!parsed.require_v2);
}

#[test]
fn the_store_sets_and_clears_it_per_session_and_the_verifier_reads_it() {
    let store = Store::open_in_memory().unwrap();
    let now = crate::time::epoch_secs();
    let (k_a, k_b) = (key(64), key(65));
    let (a, b) = (writer(&k_a, "a", now), writer(&k_b, "b", now));
    store.attestation_register(&a).unwrap();
    store.attestation_register(&b).unwrap();

    store.attestation_set_require_v2("b", true).unwrap();
    assert!(!store.attestation_binding("a").unwrap().unwrap().require_v2);
    assert!(store.attestation_binding("b").unwrap().unwrap().require_v2);
    let listed: Vec<(String, bool)> = store
        .attestation_bindings()
        .unwrap()
        .into_iter()
        .map(|x| (x.session, x.require_v2))
        .collect();
    assert_eq!(listed, [("a".into(), false), ("b".into(), true)]);

    // Through the store as the binding source, as the HTTP pre-check reads it.
    let check = |k: &Ed25519KeyPair, x: &SessionBinding, v2: bool| {
        let (envelope, payload) = signed(k, x, v2, now);
        check_binding_deferred(&store, &envelope, &payload, now, ATTESTATION_SKEW_SECS)
            .map(|_| ())
            .map_err(|r| r.verdict)
    };
    assert_eq!(check(&k_a, &a, false), Ok(()));
    assert_eq!(check(&k_b, &b, false), Err("invalid"));
    assert_eq!(check(&k_b, &b, true), Ok(()));

    store.attestation_set_require_v2("b", false).unwrap();
    assert_eq!(check(&k_b, &b, false), Ok(()), "allow-v1 restores v1");
    assert!(store.attestation_set_require_v2("nobody", true).is_err());
}

#[test]
fn a_binding_that_predates_the_column_migrates_as_v1_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pre-require-v2.db");
    let now = crate::time::epoch_secs();
    let k = key(66);
    let b = writer(&k, "legacy-writer", now);
    {
        // The schema as it stood after aegis-bys8d1 and before this column.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE attestation_bindings (
                 session TEXT PRIMARY KEY, agent TEXT NOT NULL, public_key TEXT NOT NULL,
                 key_id TEXT NOT NULL UNIQUE, introducer TEXT NOT NULL,
                 issued_at_epoch INTEGER NOT NULL, expires_at_epoch INTEGER NOT NULL,
                 revoked INTEGER NOT NULL DEFAULT 0 CHECK (revoked IN (0, 1)),
                 allow_write INTEGER NOT NULL DEFAULT 0 CHECK (allow_write IN (0, 1)));",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attestation_bindings VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 1)",
            rusqlite::params![
                b.session,
                b.agent,
                b.public_key,
                b.key_id,
                b.introducer,
                i64::try_from(b.issued_at_epoch).unwrap(),
                i64::try_from(b.expires_at_epoch).unwrap()
            ],
        )
        .unwrap();
    }
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let read = store.attestation_binding("legacy-writer").unwrap().unwrap();
    assert!(read.allow_write, "control: the legacy row was read at all");
    assert!(
        !read.require_v2,
        "an existing writer must keep signing v1 until an operator moves it"
    );
    let (envelope, payload) = signed(&k, &b, false, now);
    assert!(
        check_binding_deferred(&store, &envelope, &payload, now, ATTESTATION_SKEW_SECS).is_ok()
    );
    // The migrated column takes the setter, so the ALTER really added it.
    store
        .attestation_set_require_v2("legacy-writer", true)
        .unwrap();
    assert!(
        check_binding_deferred(&store, &envelope, &payload, now, ATTESTATION_SKEW_SECS).is_err()
    );
}
