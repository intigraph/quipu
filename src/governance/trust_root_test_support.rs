//! Test support for the trust root (aegis-kzt0ql.9.4): a deterministic TEST
//! trust root, enrolled through the real `bootstrap`, signing real
//! amendments. Not a bypass: nothing here writes the registry except through
//! the gate.

use super::*;

/// The verifier name of the test trust root.
pub(crate) const TEST_ROOT: &str = "test-root";

fn root_key() -> ring::signature::Ed25519KeyPair {
    ring::signature::Ed25519KeyPair::from_seed_unchecked(&[7u8; 32]).expect("seed")
}

/// Enrol the test root in `store` if nothing has been enrolled yet.
pub(crate) fn ensure_root(store: &mut Store) {
    if ever_bootstrapped(store).unwrap() {
        return;
    }
    let kp = root_key();
    let key = crate::signing::public_key_hex(&kp);
    let pop = crate::signing::sign_hex(
        &kp,
        &bootstrap_message(&store.store_id().unwrap(), TEST_ROOT, &key),
    );
    bootstrap(store, TEST_ROOT, &key, &[], &pop, "1970-01-01T00:00:00Z").unwrap();
}

/// Apply `change` to human-tier `registration` at `timestamp`, signed by the
/// test root as a real amendment.
pub(crate) fn amend(store: &mut Store, registration: &str, change: Vec<Datum>, timestamp: &str) {
    ensure_root(store);
    let digest = digest_after(store, registration, &change).unwrap();
    let mut n = [0u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut n).unwrap();
    let nonce = hex::encode(n);
    let sig = crate::signing::sign_hex(
        &root_key(),
        &amendment_message(&store.store_id().unwrap(), registration, &digest, &nonce),
    );
    let a = format!("http://ex/test-amendment/{nonce}");
    let d = |store: &Store, p: &str, v: Value| Datum {
        entity: store.intern(&a).unwrap(),
        attribute: store.intern(p).unwrap(),
        value: v,
        valid_from: timestamp.to_string(),
        valid_to: None,
        op: Op::Assert,
    };
    let mut datums = change;
    datums.extend([
        d(
            store,
            RDF_TYPE,
            Value::Ref(store.intern(&ns("RegistryAmendment")).unwrap()),
        ),
        d(
            store,
            &ns("amends"),
            Value::Ref(store.intern(registration).unwrap()),
        ),
        d(store, &ns("amendedDigest"), Value::Str(digest)),
        d(store, &ns("amendmentSigner"), Value::Str(TEST_ROOT.into())),
        d(store, &ns("amendmentNonce"), Value::Str(nonce)),
        d(store, &ns("amendmentSignature"), Value::Str(sig)),
    ]);
    store.transact(&datums, timestamp, None, None).unwrap();
}

/// Register `verifier`'s `key` for `policies` as a HUMAN-tier registration.
pub(crate) fn register_human(
    store: &mut Store,
    registration: &str,
    verifier: &str,
    policies: &[&str],
    key: &str,
    timestamp: &str,
) {
    let d = |store: &Store, p: &str, v: Value| Datum {
        entity: store.intern(registration).unwrap(),
        attribute: store.intern(p).unwrap(),
        value: v,
        valid_from: timestamp.to_string(),
        valid_to: None,
        op: Op::Assert,
    };
    let mut change = vec![
        d(
            store,
            RDF_TYPE,
            Value::Ref(store.intern(&ns("VerifierRegistration")).unwrap()),
        ),
        d(store, &ns("verifier"), Value::Str(verifier.into())),
        d(store, &ns("publicKey"), Value::Str(key.into())),
        d(store, TRUST_TIER, Value::Str(HUMAN_TIER.into())),
    ];
    for p in policies {
        change.push(d(store, &ns("attests"), Value::Str((*p).into())));
    }
    amend(store, registration, change, timestamp);
}

/// Endorse an EXISTING registration as `tier` (aegis-c97o): the trust root
/// signs an amendment adding the ROOT `aegis:trustTier` marker. This is the
/// migration step for a registration written before the fix.
pub(crate) fn govern(store: &mut Store, registration: &str, tier: &str, timestamp: &str) {
    let change = vec![Datum {
        entity: store.intern(registration).unwrap(),
        attribute: store.intern(TRUST_TIER).unwrap(),
        value: Value::Str(tier.into()),
        valid_from: timestamp.to_string(),
        valid_to: None,
        op: Op::Assert,
    }];
    amend(store, registration, change, timestamp);
}

/// Retract `registration`'s `predicate` = `value` (asserted from
/// `asserted_from`) at `timestamp` through a root-signed amendment: rotation
/// and revocation of a GOVERNED registration go through the gate like any
/// other change.
pub(crate) fn amend_retract(
    store: &mut Store,
    registration: &str,
    predicate: &str,
    value: Value,
    asserted_from: &str,
    timestamp: &str,
) {
    let change = vec![Datum {
        entity: store.intern(registration).unwrap(),
        attribute: store.intern(predicate).unwrap(),
        value,
        valid_from: asserted_from.to_string(),
        valid_to: None,
        op: Op::Retract,
    }];
    amend(store, registration, change, timestamp);
}
