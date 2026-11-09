//! Fixture-only composition of native reservation, signature and slot evidence.
//! No endpoint or production enable switch calls this module.
use super::{
    Store,
    slot_read_batch::{BatchReadLimits, BatchViews},
};
use crate::resolver_contract::{ContentDigest, Slot, SourceBinding};
use crate::session_attestation::{
    self as attest, AttestationEnvelope, SignedBinding, WriteBinding,
};
use crate::write_base::{LegacyBasePolicy, WriterSnapshot, prepare_slot_reads};
use crate::{Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

const PATH: &str = "/__fixture__/reserved-admission";
const SHAPES: &str = "@prefix sh:<http://www.w3.org/ns/shacl#>. <urn:S> a sh:NodeShape; sh:targetSubjectsOf <urn:p>; sh:property [sh:path <urn:p>; sh:maxCount 1].";
const TS: &str = "2026-01-01T00:00:00Z";

#[derive(Clone, Serialize)]
struct Proposal {
    source: SourceBinding,
    touched: Vec<Slot>,
    valid_at: String,
    /// Complete incoming sets, canonical RDF terms, in touched-slot order.
    incoming: Vec<Vec<String>>,
    profile: ContentDigest,
}

/// Only the fresh, private fixture producer can mint this genesis witness.
/// It is NOT a production retention watermark or a transferable capability.
struct FixtureHistory {
    store: String,
    epoch: String,
}
impl FixtureHistory {
    fn fresh(store: &Store) -> Result<Self> {
        if store.latest_tx_id()? != 0 {
            return Err(refused("not genesis"));
        }
        Ok(Self {
            store: store.store_id()?,
            epoch: store.log_epoch()?,
        })
    }
}

fn refused(reason: &str) -> Error {
    Error::CannotVerify(format!("reserved admission fixture: {reason}"))
}

fn profile(store: &Store) -> Result<ContentDigest> {
    if store.list_shapes()? != vec![("one".into(), SHAPES.into(), TS.into())]
        || !store.list_ontologies()?.is_empty()
        || !store.shacl_config.validate_on_write
        || store.recording_verdicts
    {
        return Err(refused("unsupported validator profile"));
    }
    Ok(ContentDigest(Sha256::digest(SHAPES.as_bytes()).into()))
}

fn snapshot(store: &Store, history: &FixtureHistory) -> Result<WriterSnapshot> {
    if history.store != store.store_id()? || history.epoch != store.log_epoch()? {
        return Err(refused("history witness identity"));
    }
    Ok(WriterSnapshot {
        store_id: history.store.clone(),
        log_epoch: history.epoch.clone(),
        graph: crate::schema::ROOT_GRAPH_IRI.into(),
        oldest_complete_tx: 0,
        head_tx: u64::try_from(store.latest_tx_id()?).map_err(|_| refused("head range"))?,
    })
}

/// Hash the reconstructed base, not the declaration or current head. Canonical
/// terms avoid local interner ids; length-framed JSON preserves empty slots.
fn slice_hash(store: &Store, batch: &BatchViews) -> Result<ContentDigest> {
    let mut slots = Vec::new();
    for view in &batch.slots {
        let mut values = view
            .base
            .iter()
            .map(|f| Ok(crate::rdf::value_to_term(store, &f.value)?.to_string()))
            .collect::<Result<Vec<_>>>()?;
        values.sort();
        values.dedup();
        slots.push((&view.slot, values));
    }
    slots.sort_by(|a, b| (&a.0.subject, &a.0.predicate).cmp(&(&b.0.subject, &b.0.predicate)));
    let mut hash = Sha256::new();
    hash.update(b"quipu-fixture-write-base/v1\0");
    hash.update(serde_json::to_vec(&slots).map_err(|_| refused("slice encoding"))?);
    Ok(ContentDigest(hash.finalize().into()))
}

fn read(store: &Store, history: &FixtureHistory, request: &Proposal) -> Result<BatchViews> {
    let coordinates = snapshot(store, history)?;
    let prepared = prepare_slot_reads(
        Some(&request.source),
        &coordinates,
        LegacyBasePolicy::RequireDeclared,
        &request.touched,
        8,
        &request.valid_at,
    )?;
    store.read_prepared_slot_batch(
        &prepared,
        BatchReadLimits {
            claims_per_read: 64,
            bytes_per_read: 65536,
            total_claims: 1024,
            total_bytes: 1048576,
        },
    )
}

fn payload<'a>(body: &'a str, audience: &'a str) -> SignedBinding<'a> {
    SignedBinding::Write(WriteBinding {
        method: "POST",
        path: PATH,
        content_type: "application/json",
        body_sha256: body,
        audience: Some(audience),
    })
}

/// Default-off entry. Reserve BEFORE registry, nonce, policy, profile or facts.
/// A panic is converted to an error INSIDE atomic, then resumed after rollback.
fn admit(
    store: &mut Store,
    enabled: bool,
    history: &FixtureHistory,
    request: &Proposal,
    envelope: &AttestationEnvelope,
    now: u64,
) -> Result<i64> {
    if !enabled {
        return Err(refused("disabled"));
    }
    if !request.source.inputs.is_empty()
        || !store.conn.is_autocommit()
        || request.touched.is_empty()
        || request.incoming.len() != request.touched.len()
        || request.incoming.iter().any(|v| v.len() > 8)
    {
        return Err(refused("entry or request bounds"));
    }
    let bytes = serde_json::to_vec(request).map_err(|_| refused("request encoding"))?;
    if bytes.len() > 65536 {
        return Err(refused("request bytes"));
    }
    let body = attest::body_sha256(&bytes);
    let mut panic = None;
    let prior_chain = store.principal_chain().to_vec();
    let result = crate::share_merge::core::atomic(store, |store| {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let audience = store.store_id()?;
            let principal =
                attest::verify_binding(store, envelope, &payload(&body, &audience), now, 300)?;
            if !store.governance_config.enforce_authority || store.recording_verdicts {
                return Err(refused("authority enforcement required"));
            }
            // Preserve and narrow an existing delegation chain; never replace it
            // with the signer alone, nor grant a named-graph capability ROOT.
            let mut chain = prior_chain.clone();
            chain.push(principal.agent.clone());
            store.set_principal_chain(chain);
            store.enforce_graph_authority(crate::schema::ROOT_GRAPH)?;
            if profile(store)? != request.profile {
                return Err(refused("profile binding"));
            }
            let batch = read(store, history, request)?;
            if slice_hash(store, &batch)? != request.source.base_slice_hash {
                return Err(refused("base slice hash"));
            }
            let identity = crate::transaction_auth::Identity {
                principal: principal.agent,
                credential_id: Some(principal.key_id),
                auth_class: "fixture-native-v2".into(),
                attestation: Default::default(),
            };
            crate::transaction_auth::with_identity(Some(identity), || apply(store, request, &batch))
        }));
        store.set_principal_chain(prior_chain.clone());
        match outcome {
            Ok(r) => r,
            Err(p) => {
                panic = Some(p);
                Err(refused("unwind"))
            }
        }
    });
    if let Some(p) = panic {
        std::panic::resume_unwind(p);
    }
    result
}

mod apply;
use apply::apply;
#[path = "reserved_admission_fixture/tests.rs"]
mod tests;
