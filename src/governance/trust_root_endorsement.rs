//! Endorsing agent registrations and reporting a registration's trust-root
//! chain (aegis-c97o). Split from `trust_root.rs` for the file-size ratchet;
//! re-exported there.

use super::*;

/// How a governed registration chains to the trust root, for a verifier to
/// report (aegis-c97o). This is PROVENANCE, not the trust decision: trust
/// comes from the registration being governed at the witness, which only the
/// gate can make true. The chain is re-checked here (the amendment's
/// signature against a current human trust-root key, the bootstrap record's
/// fingerprint against the registration's key) so a hand-written record
/// cannot be reported as the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chain {
    /// Enrolled by the console bootstrap; `record` is the
    /// `aegis:TrustRootBootstrap` and `fingerprint` the key it enrolled.
    Bootstrap {
        /// The bootstrap record.
        record: String,
        /// The enrolled key's fingerprint.
        fingerprint: String,
    },
    /// Last admitted by `amendment`, signed by human trust-root `signer`.
    Amendment {
        /// The `aegis:RegistryAmendment`.
        amendment: String,
        /// The human trust-root verifier that signed it.
        signer: String,
    },
    /// Governed, but no amendment found verifies under a CURRENT human
    /// trust-root key (the root has rotated since). The gate admitted it.
    GateOnly,
}

/// Find the trust-root chain of `registration` (see [`Chain`]).
pub fn chain_of(store: &Store, registration: &str) -> Result<Chain> {
    let rdf_type = store.lookup(RDF_TYPE)?;
    // Bootstrap: a record naming this registration whose fingerprint is the
    // fingerprint of one of the registration's keys.
    if let (Some(t), Some(class), Some(enrolled), Some(reg)) = (
        rdf_type,
        store.lookup(&ns("TrustRootBootstrap"))?,
        store.lookup(&ns("enrolled"))?,
        store.lookup(registration)?,
    ) {
        let keys = strings(store, registration, &ns("publicKey"))?;
        let mut stmt = store.prepare(
            "SELECT DISTINCT f.e FROM facts f JOIN facts c ON c.e = f.e AND c.a = ?3 \
             AND c.v = ?4 AND c.g = 0 AND c.op = 1 AND c.valid_to IS NULL \
             WHERE f.a = ?1 AND f.v = ?2 AND f.g = 0 AND f.op = 1 AND f.valid_to IS NULL",
        )?;
        let records = stmt
            .query_map(
                params![
                    enrolled,
                    Value::Ref(reg).to_bytes(),
                    t,
                    Value::Ref(class).to_bytes()
                ],
                |row| row.get::<_, i64>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for id in records {
            let record = store.resolve(id)?;
            let Some(fp) = sole(strings(store, &record, &ns("keyFingerprint"))?) else {
                continue;
            };
            if keys.iter().any(|k| fingerprint(k).is_ok_and(|f| f == fp)) {
                return Ok(Chain::Bootstrap {
                    record,
                    fingerprint: fp,
                });
            }
        }
    }
    // Amendment: the most recent one naming this registration whose
    // signature verifies under a current human trust-root key.
    let (Some(t), Some(class), Some(amends), Some(reg)) = (
        rdf_type,
        store.lookup(&ns("RegistryAmendment"))?,
        store.lookup(&ns("amends"))?,
        store.lookup(registration)?,
    ) else {
        return Ok(Chain::GateOnly);
    };
    let mut stmt = store.prepare(
        "SELECT f.e, MAX(f.tx) FROM facts f JOIN facts c ON c.e = f.e AND c.a = ?3 \
         AND c.v = ?4 AND c.op = 1 AND c.valid_to IS NULL \
         WHERE f.a = ?1 AND f.v = ?2 AND f.op = 1 AND f.valid_to IS NULL \
         GROUP BY f.e ORDER BY MAX(f.tx) DESC",
    )?;
    let candidates = stmt
        .query_map(
            params![
                amends,
                Value::Ref(reg).to_bytes(),
                t,
                Value::Ref(class).to_bytes()
            ],
            |row| row.get::<_, i64>(0),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let amenders = snapshot(store)?.amenders;
    let store_id = store.store_id()?;
    for id in candidates {
        let iri = store.resolve(id)?;
        let (Some(digest), Some(signer), Some(nonce), Some(signature)) = (
            sole(strings(store, &iri, &ns("amendedDigest"))?),
            sole(strings(store, &iri, &ns("amendmentSigner"))?),
            sole(strings(store, &iri, &ns("amendmentNonce"))?),
            sole(strings(store, &iri, &ns("amendmentSignature"))?),
        ) else {
            continue;
        };
        let message = amendment_message(&store_id, registration, &digest, &nonce);
        if amenders.iter().any(|(verifier, key)| {
            *verifier == signer && crate::signing::verify_hex(key, &message, &signature)
        }) {
            return Ok(Chain::Amendment {
                amendment: iri,
                signer,
            });
        }
    }
    Ok(Chain::GateOnly)
}

// ── Endorsing an agent registration (aegis-c97o) ───────────────────────────
//
// Verdict verification trusts only GOVERNED registrations. A registration
// written before that rule (or by any writer since) is endorsed by the trust
// root: an amendment, signed by an enrolled human key, that adds the ROOT
// `aegis:trustTier` marker. These two helpers are the console path
// (`quipu trust-root endorse-challenge` / `endorse`); a client may equally
// build the same amendment itself with [`digest_after`].

/// What the root signs to endorse a registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndorseChallenge {
    /// The store the amendment is bound to.
    pub store_id: String,
    /// The tier being granted.
    pub tier: String,
    /// The public keys the registration holds now: what the root vouches for.
    pub keys: Vec<String>,
    /// The digest the registration will have once endorsed.
    pub digest: String,
    /// A fresh single-use nonce.
    pub nonce: String,
    /// The exact bytes to sign.
    pub message: Vec<u8>,
}

fn endorsement_marker(store: &Store, registration: &str, tier: &str, ts: &str) -> Result<Datum> {
    if tier != AGENT_TIER && tier != HUMAN_TIER {
        return Err(Error::InvalidValue(format!(
            "trust tier must be \"{AGENT_TIER}\" or \"{HUMAN_TIER}\", not \"{tier}\""
        )));
    }
    if !is_registration(store, registration)? {
        return Err(Error::InvalidValue(format!(
            "{registration} is not an aegis:VerifierRegistration in ROOT"
        )));
    }
    Ok(Datum {
        entity: store.intern(registration)?,
        attribute: store.intern(TRUST_TIER)?,
        value: Value::Str(tier.into()),
        valid_from: ts.to_string(),
        valid_to: None,
        op: Op::Assert,
    })
}

/// The challenge a human trust-root key signs to endorse `registration` as
/// `tier` (normally `"agent"`).
pub fn endorse_challenge(
    store: &Store,
    registration: &str,
    tier: &str,
) -> Result<EndorseChallenge> {
    let marker = endorsement_marker(store, registration, tier, "1970-01-01T00:00:00Z")?;
    let digest = digest_after(store, registration, &[marker])?;
    let mut n = [0u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut n)
        .map_err(|_| Error::InvalidValue("no randomness for a nonce".into()))?;
    let nonce = hex::encode(n);
    let store_id = store.store_id()?;
    let message = amendment_message(&store_id, registration, &digest, &nonce);
    Ok(EndorseChallenge {
        store_id,
        tier: tier.to_string(),
        keys: strings(store, registration, &ns("publicKey"))?,
        digest,
        nonce,
        message,
    })
}

/// Write the endorsement: the tier marker plus the signed
/// `aegis:RegistryAmendment`, in one transaction. The gate verifies it; this
/// function checks nothing the gate does not. Returns the amendment IRI.
pub fn endorse(
    store: &mut Store,
    registration: &str,
    tier: &str,
    signer: &str,
    nonce: &str,
    signature: &str,
    timestamp: &str,
) -> Result<String> {
    let marker = endorsement_marker(store, registration, tier, timestamp)?;
    let digest = digest_after(store, registration, std::slice::from_ref(&marker))?;
    let amendment = ns(&format!("registry_amendment_{nonce}"));
    let d = |store: &Store, p: &str, v: Value| -> Result<Datum> {
        Ok(Datum {
            entity: store.intern(&amendment)?,
            attribute: store.intern(p)?,
            value: v,
            valid_from: timestamp.to_string(),
            valid_to: None,
            op: Op::Assert,
        })
    };
    let datums = vec![
        marker,
        d(
            store,
            RDF_TYPE,
            Value::Ref(store.intern(&ns("RegistryAmendment"))?),
        )?,
        d(
            store,
            &ns("amends"),
            Value::Ref(store.intern(registration)?),
        )?,
        d(store, &ns("amendedDigest"), Value::Str(digest))?,
        d(store, &ns("amendmentSigner"), Value::Str(signer.into()))?,
        d(store, &ns("amendmentNonce"), Value::Str(nonce.into()))?,
        d(
            store,
            &ns("amendmentSignature"),
            Value::Str(signature.into()),
        )?,
    ];
    store.transact(
        &datums,
        timestamp,
        Some("console"),
        Some("trust-root endorse"),
    )?;
    Ok(amendment)
}

/// ROOT `aegis:VerifierRegistration`s with no governed trust tier now, as
/// (registration, verifier). These verify nothing until endorsed.
pub fn unendorsed(store: &Store) -> Result<Vec<(String, String)>> {
    let governed = governed_registry(store)?;
    let Some(t) = store.lookup(RDF_TYPE)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for namespace in [DEFAULT_BASE_NS, "https://scbrown.github.io/quechua/ns#"] {
        let Some(class) = store.lookup(&format!("{namespace}VerifierRegistration"))? else {
            continue;
        };
        let mut stmt = store.prepare(
            "SELECT DISTINCT e FROM facts WHERE a = ?1 AND v = ?2 AND g = 0 \
             AND op = 1 AND valid_to IS NULL",
        )?;
        let ids = stmt
            .query_map(params![t, Value::Ref(class).to_bytes()], |row| {
                row.get::<_, i64>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for id in ids {
            let iri = store.resolve(id)?;
            if governed.contains_key(&iri) || out.iter().any(|(r, _)| *r == iri) {
                continue;
            }
            let verifier = sole(strings(store, &iri, &ns("verifier"))?).unwrap_or_default();
            out.push((iri, verifier));
        }
    }
    Ok(out)
}
