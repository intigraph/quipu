//! Source-only witness proofs. No bundle-supplied field creates a witness grant.
//! Native capture requires actual local authenticated origin, including bearer
//! origins; writers do not need individual event signatures. Receiver admission
//! and persistence must still use the normal native writer, not these wire bytes.
use super::{Observation, Policy, invalid, native_registry, portable, writer};
use crate::{Result, Store};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CONTRACT: &str = "quipu-fluent-source-witness/v1";
const DOMAIN: &[u8] = b"quipu-fluent-source-witness/v1\0";
const MAX_PROOF: usize = 131_072;
const MAX_FIELD: usize = 16_384;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Slot {
    graph: String,
    fluent: String,
    subject: String,
    predicate: String,
    property_shape: String,
    policy: Policy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Origin {
    contract: String,
    source_store: String,
    profile_sha256: String,
    slot: Slot,
    observation: Observation,
    credential_id: Option<String>,
    auth_class: String,
}

/// This value can only be obtained by reading actual native source evidence.
/// It deliberately has no Deserialize or caller-field constructor.
pub(super) struct LocalOrigin(Origin);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Proof {
    body: Origin,
    witness: String,
    signature: String,
}

/// A receiver-local, separately governed grant; never part of the wire proof.
/// Production construction uses the ROOT governed verifier registry, with a
/// domain-separated scope binding the complete source/declaration/profile.
/// This module never registers a key or grants a capability.
#[derive(Clone)]
struct WitnessGrant {
    witness: String,
    source_store: String,
    slot: Slot,
    profile_sha256: String,
    public_key_hex: String,
    revoked: bool,
}

/// An origin proof, not a receiver write permit or a local transaction identity.
/// A relay retains the original proof; it must not replace it with importer text.
#[derive(Debug)]
pub(super) struct Verified {
    observation: Observation,
    original_proof: Proof,
}

fn profile(store: &Store) -> Result<String> {
    let shapes = store
        .get_combined_shapes()?
        .ok_or_else(|| invalid("source witness requires loaded shapes"))?;
    Ok(hex::encode(Sha256::digest(shapes.as_bytes())))
}

pub(super) fn capture(
    store: &Store,
    graph: i64,
    fluent: &str,
    event: &str,
    expected_source: &str,
) -> Result<LocalOrigin> {
    let source_store = store.store_id()?;
    writer::iri(&source_store)?;
    if expected_source != source_store {
        return Err(invalid(
            "source witness store identity does not match this store",
        ));
    }
    // Validates descriptors, graph binding, exact loaded native profile and
    // native-origin evidence before selecting a record from the protected log.
    let (_registry, log) = native_registry::load(store, graph)?;
    let observation = log
        .observations(fluent)
        .into_iter()
        .find(|o| o.order.event == event)
        .ok_or_else(|| invalid("source witness cannot attest an unadmitted event"))?
        .clone();
    let event_id = store
        .lookup(event)?
        .ok_or_else(|| invalid("missing source event"))?;
    if store.fluent_origin_receipt(graph, event_id)?.is_some() {
        return Err(invalid(
            "transported event must retain its original source proof",
        ));
    }
    let entity = store
        .lookup(fluent)?
        .ok_or_else(|| invalid("missing fluent"))?;
    let rows = store.entity_facts_in_graph(entity, graph)?;
    let field = |name: &str| -> Result<String> {
        let attribute = store
            .lookup(&format!("{}{name}", crate::namespace::QUIPU))?
            .ok_or_else(|| invalid("source descriptor field missing"))?;
        let values: Vec<_> = rows.iter().filter(|f| f.attribute == attribute).collect();
        let [fact] = values.as_slice() else {
            return Err(invalid("source descriptor field ambiguous"));
        };
        let crate::Value::Ref(value) = fact.value else {
            return Err(invalid("source descriptor field is not an IRI"));
        };
        store.resolve(value)
    };
    let binding = writer::binding_for(
        store,
        graph,
        &field("slotSubject")?,
        &field("slotPredicate")?,
        &field("propertyShape")?,
    )?;
    let auth = store
        .transaction_auth(
            i64::try_from(observation.order.tx).map_err(|_| invalid("origin tx overflow"))?,
        )?
        .ok_or_else(|| invalid("source witness needs authenticated transaction evidence"))?;
    if auth.principal != observation.order.writer || auth.auth_class.is_empty() {
        return Err(invalid(
            "source witness authentication does not match event",
        ));
    }
    let body = Origin {
        contract: CONTRACT.into(),
        source_store,
        profile_sha256: profile(store)?,
        slot: Slot {
            graph: binding.graph,
            fluent: fluent.into(),
            subject: binding.subject,
            predicate: binding.declaration.predicate,
            property_shape: binding.declaration.shape,
            policy: binding.declaration.policy,
        },
        observation,
        credential_id: auth.credential_id,
        auth_class: auth.auth_class,
    };
    validate(&body)?;
    Ok(LocalOrigin(body))
}

fn validate(body: &Origin) -> Result<()> {
    if body.contract != CONTRACT
        || body.slot.fluent != body.observation.fluent
        || body.slot.fluent == body.observation.order.event
    {
        return Err(invalid("source witness contract or fluent mismatch"));
    }
    for text in [
        &body.source_store,
        &body.slot.graph,
        &body.slot.fluent,
        &body.slot.subject,
        &body.slot.predicate,
        &body.slot.property_shape,
    ] {
        if text.len() > MAX_FIELD {
            return Err(invalid("source witness identity exceeds bound"));
        }
        writer::iri(text)?;
    }
    if body.auth_class.is_empty()
        || body.auth_class.len() > MAX_FIELD
        || body
            .credential_id
            .as_ref()
            .is_some_and(|s| s.len() > MAX_FIELD)
        || body.profile_sha256.len() != 64
        || !body
            .profile_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("invalid source authentication or profile digest"));
    }
    let observation = &body.observation;
    i64::try_from(observation.order.tx)
        .map_err(|_| invalid("source event transaction exceeds native representation"))?;
    for text in [
        &observation.order.writer,
        &observation.order.event,
        &observation.as_of,
        &observation.recorded_on,
    ] {
        if text.len() > MAX_FIELD {
            return Err(invalid("source event field exceeds bound"));
        }
    }
    if matches!(&observation.value, super::Scalar::Term(text) if text.len() > MAX_FIELD) {
        return Err(invalid("source event value exceeds bound"));
    }
    portable::validate(&body.observation)?;
    let mut log = super::Log::default();
    log.append(body.observation.clone())?;
    log.current(&body.slot.fluent, body.slot.policy)?;
    // Count all event/time/value/credential fields as well as identity fields.
    if serde_json::to_vec(body)
        .map_err(|_| invalid("source witness encoding"))?
        .len()
        > MAX_PROOF / 2
    {
        return Err(invalid("source witness body exceeds bound"));
    }
    Ok(())
}

fn message(body: &Origin, witness: &str) -> Result<Vec<u8>> {
    validate(body)?;
    if witness.len() > MAX_FIELD {
        return Err(invalid("witness identity exceeds bound"));
    }
    writer::iri(witness)?;
    let mut bytes = DOMAIN.to_vec();
    bytes.extend(
        serde_json::to_vec(&(witness, body)).map_err(|_| invalid("source witness encoding"))?,
    );
    Ok(bytes)
}

/// Called by a separately admitted source witness, never by generic bundle IO.
pub(super) fn seal(
    origin: LocalOrigin,
    witness: &str,
    signer: &quipu_signing::SigningIdentity,
) -> Result<Proof> {
    let signature = signer.sign(&message(&origin.0, witness)?);
    Ok(Proof {
        body: origin.0,
        witness: witness.into(),
        signature,
    })
}

impl Proof {
    /// Exact capability to enroll through the existing trust-root amendment
    /// path. No wildcard producer role, new RDF class, or carried registration
    /// can authorize this identifier.
    pub(super) fn scope_id(&self) -> Result<String> {
        validate(&self.body)?;
        let mut bytes = b"quipu-fluent-source-witness-scope/v1\0".to_vec();
        bytes.extend(
            serde_json::to_vec(&(
                &self.body.source_store,
                &self.body.profile_sha256,
                &self.body.slot,
            ))
            .map_err(|_| invalid("source witness scope encoding"))?,
        );
        Ok(format!(
            "urn:quipu:fluent-source-witness-scope:v1:{}",
            hex::encode(Sha256::digest(bytes))
        ))
    }

    /// A current proof check, not a native write permit. The future import
    /// materializer must invoke this again under its normal writer lock.
    /// Sender-provided recording times never select a historical grant.
    pub(super) fn verify_governed_now(&self, store: &Store) -> Result<Verified> {
        self.verify_governed_at(store, &crate::governance::verifier_registry::Witness::now())
    }

    fn verify_governed_at(
        &self,
        store: &Store,
        when: &crate::governance::verifier_registry::Witness,
    ) -> Result<Verified> {
        use crate::governance::verifier_registry::{self, Scope};
        message(&self.body, &self.witness)?;
        if self.signature.len() != 128 {
            return Err(invalid("invalid source witness signature size"));
        }
        let scope = self.scope_id()?;
        let registrations = verifier_registry::keyed_registrations(
            store,
            &self.witness,
            Some(&scope),
            when,
            Scope::Governed,
        )?;
        if registrations.len() != 1 {
            return Err(invalid(
                "governed source witness registration missing or ambiguous",
            ));
        }
        let grant = WitnessGrant {
            witness: self.witness.clone(),
            source_store: self.body.source_store.clone(),
            slot: self.body.slot.clone(),
            profile_sha256: self.body.profile_sha256.clone(),
            public_key_hex: registrations[0].key.clone(),
            revoked: false,
        };
        self.verify(&[grant], &self.body.slot.graph, &self.body.slot.fluent)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_PROOF {
            return Err(invalid("source witness envelope exceeds bound"));
        }
        let proof: Self =
            serde_json::from_slice(bytes).map_err(|_| invalid("malformed source witness proof"))?;
        message(&proof.body, &proof.witness)?;
        if proof.signature.len() != 128 {
            return Err(invalid("invalid source witness signature size"));
        }
        Ok(proof)
    }

    fn verify(
        &self,
        grants: &[WitnessGrant],
        expected_graph: &str,
        expected_fluent: &str,
    ) -> Result<Verified> {
        let bytes = message(&self.body, &self.witness)?;
        if self.signature.len() != 128
            || self.body.slot.graph != expected_graph
            || self.body.slot.fluent != expected_fluent
        {
            return Err(invalid("source witness target or signature mismatch"));
        }
        let matches: Vec<_> = grants
            .iter()
            .filter(|g| {
                g.witness == self.witness
                    && g.source_store == self.body.source_store
                    && g.slot == self.body.slot
                    && g.profile_sha256 == self.body.profile_sha256
            })
            .collect();
        let [grant] = matches.as_slice() else {
            return Err(invalid("source witness grant missing or ambiguous"));
        };
        if grant.revoked
            || !quipu_signing::verify_hex(&grant.public_key_hex, &bytes, &self.signature)
        {
            return Err(invalid("source witness revoked or signature invalid"));
        }
        Ok(Verified {
            observation: self.body.observation.clone(),
            original_proof: self.clone(),
        })
    }
}

#[cfg(all(test, feature = "shacl"))]
#[path = "source_witness_tests.rs"]
mod tests;

#[cfg(all(test, feature = "shacl"))]
#[path = "source_witness_governed_tests.rs"]
mod governed_tests;

#[path = "source_import.rs"]
mod source_import;
pub(super) use source_import::imported_origin;

#[path = "source_batch.rs"]
mod source_batch;
pub(crate) use source_batch::{materialize_batch, persist_batch};

#[cfg(all(test, feature = "shacl"))]
#[path = "source_batch_tests.rs"]
mod batch_tests;

#[cfg(all(test, feature = "shacl"))]
#[path = "source_import_tests.rs"]
mod import_tests;

#[cfg(all(test, feature = "shacl", not(target_arch = "wasm32")))]
#[path = "source_carrier_tests.rs"]
mod carrier_tests;

#[cfg(all(test, feature = "shacl", not(target_arch = "wasm32")))]
#[path = "source_transport_tests.rs"]
mod transport_tests;

#[cfg(all(test, feature = "shacl", not(target_arch = "wasm32")))]
#[path = "source_boundary_tests.rs"]
mod boundary_tests;

#[cfg(all(test, feature = "shacl", not(target_arch = "wasm32")))]
#[path = "source_pendant_tests.rs"]
mod pendant_tests;
