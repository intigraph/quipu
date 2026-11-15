//! Non-executable configuration requests, distinct from source-event evidence.
//! No decoder or generic bundle loader registers an observable or enrolls keys.
use super::{Observable, Policy, invalid, registration::Register, writer};
use crate::{Result, Store};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CONTRACT: &str = "quipu-pending-observable-config/v1";
const MAX_BYTES: usize = 65_536;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    contract: String,
    graph: String,
    name: String,
    subject: String,
    predicate: String,
    property_shape: String,
    policy: Policy,
    observes: Vec<String>,
}

/// Exact retained request bytes; syntax validation is never acceptance.
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    bytes: Vec<u8>,
    descriptor: Descriptor,
}

impl Pending {
    pub(crate) fn from_registration(request: &Register) -> Result<Self> {
        if request.observable.observes.len() > 128
            || [
                &request.observable.graph,
                &request.observable.name,
                &request.subject,
                &request.predicate,
                &request.property_shape,
            ]
            .into_iter()
            .chain(request.observable.observes.iter())
            .any(|field| field.len() > 4096)
        {
            return Err(invalid(
                "configuration request exceeds field/dependency bound",
            ));
        }
        let body = Descriptor {
            contract: CONTRACT.into(),
            graph: request.observable.graph.clone(),
            name: request.observable.name.clone(),
            subject: request.subject.clone(),
            predicate: request.predicate.clone(),
            property_shape: request.property_shape.clone(),
            policy: request.observable.computed_by,
            observes: request.observable.observes.iter().cloned().collect(),
        };
        let bytes = serde_json::to_vec(&body).map_err(|_| invalid("config request encoding"))?;
        Self::decode(&bytes)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err(invalid("pending config exceeds byte bound"));
        }
        let descriptor: Descriptor = serde_json::from_slice(bytes)
            .map_err(|_| invalid("malformed pending configuration request"))?;
        if descriptor.contract != CONTRACT
            || descriptor.observes.is_empty()
            || descriptor.observes.len() > 128
            || descriptor
                .observes
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(invalid("invalid config contract or sorted dependencies"));
        }
        for iri in [
            &descriptor.graph,
            &descriptor.name,
            &descriptor.subject,
            &descriptor.predicate,
            &descriptor.property_shape,
        ]
        .into_iter()
        .chain(descriptor.observes.iter())
        {
            if iri.len() > 4096 {
                return Err(invalid("config identity exceeds bound"));
            }
            writer::iri(iri)?;
        }
        Ok(Self {
            bytes: bytes.to_vec(),
            descriptor,
        })
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn id(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"quipu-pending-observable-config/v1\0");
        digest.update(&self.bytes);
        format!(
            "urn:quipu:pending-observable-config:v1:{}",
            hex::encode(digest.finalize())
        )
    }

    pub(super) fn registration(&self) -> Register {
        let body = &self.descriptor;
        Register {
            observable: Observable {
                graph: body.graph.clone(),
                name: body.name.clone(),
                computed_by: body.policy,
                observes: body.observes.iter().cloned().collect(),
            },
            subject: body.subject.clone(),
            predicate: body.predicate.clone(),
            property_shape: body.property_shape.clone(),
        }
    }

    pub(crate) fn materialize(
        &self,
        approval: &Approval,
        store: &Store,
        graph: i64,
        tx: i64,
        timestamp: &str,
    ) -> Result<writer::Prepared> {
        approval.verify(store, self, graph)?;
        let prepared = self
            .registration()
            .materialize(store, graph, tx, timestamp)?;
        if let writer::Prepared::Replay(original_tx) = prepared {
            let receipt = self
                .receipt(store, graph)?
                .ok_or_else(|| invalid("existing config has no original acceptance receipt"))?;
            if receipt.tx != original_tx
                || receipt.request != self.bytes
                || receipt.approval != approval.bytes()?
            {
                return Err(invalid(
                    "configuration replay differs from original receipt",
                ));
            }
            return Ok(writer::Prepared::Replay(original_tx));
        }
        Ok(prepared)
    }
}

pub(super) fn profile(store: &Store) -> Result<String> {
    let shapes = store
        .get_combined_shapes()?
        .ok_or_else(|| invalid("configuration acceptance requires loaded shapes"))?;
    Ok(hex::encode(Sha256::digest(shapes.as_bytes())))
}

#[path = "pending_config_approval.rs"]
mod approval;
pub(crate) use approval::Approval;

#[path = "pending_config_storage.rs"]
pub(crate) mod storage;

#[cfg(all(test, feature = "shacl", not(target_arch = "wasm32")))]
#[path = "pending_config_tests.rs"]
mod tests;
