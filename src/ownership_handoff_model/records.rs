use super::{ModelResult, require};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Candidate record version; not an admitted transport/profile version.
pub const RECORD_VERSION: u16 = 1;
/// Maximum serialized model record size, including before JSON parsing.
pub const MAX_RECORD_BYTES: usize = 65_536;
/// SHA-256 identity of model content or an externally inspected artifact.
pub type Digest = [u8; 32];

fn identifier(value: &str) -> ModelResult<()> {
    require(
        !value.is_empty()
            && value.len() <= 512
            && value.trim() == value
            && !value.chars().any(char::is_control),
        "missing, oversized or ambiguous identifier",
    )
}

fn digest(value: &Digest) -> ModelResult<()> {
    require(*value != [0; 32], "missing digest")
}

fn hash(domain: &[u8], value: &impl Serialize) -> ModelResult<Digest> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    h.update(domain);
    h.update((bytes.len() as u64).to_be_bytes());
    h.update(bytes);
    Ok(h.finalize().into())
}

/// An already admitted principal and native-placement digest in the model.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    /// Opaque existing principal identifier, not a newly minted principal.
    pub principal: String,
    /// Digest of the native host/path binding, not proof of that binding.
    pub native: Digest,
}

impl Placement {
    pub(super) fn validate(&self) -> ModelResult<()> {
        identifier(&self.principal)?;
        digest(&self.native)
    }
}

/// This phase supports a complete logical store only; no scoped promotion.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthorityUnit {
    /// Exactly the whole store named by the enclosing tuple.
    FullStore,
}

/// Complete canonical-authority identity. An epoch alone is not authority.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    /// Logical lineage preserved across copies.
    pub store_id: String,
    /// Exact authority unit; scoped replicas cannot be promoted here.
    pub unit: AuthorityUnit,
    /// Monotonic ownership generation, never wrapping.
    pub generation: u64,
    /// Log incarnation admitted at this placement.
    pub epoch: String,
    /// Existing principal and native placement.
    pub owner: Placement,
    /// Existing policy revision, retained through this modeled handoff.
    pub policy: String,
    /// Digest of the preceding retirement, absent only at generation zero.
    pub predecessor: Option<Digest>,
}

impl Authority {
    /// Validate syntax and the genesis/predecessor relationship, not authority.
    pub fn validate(&self) -> ModelResult<()> {
        identifier(&self.store_id)?;
        identifier(&self.epoch)?;
        identifier(&self.policy)?;
        self.owner.validate()?;
        require(
            (self.generation == 0) == self.predecessor.is_none(),
            "invalid predecessor",
        )?;
        if let Some(d) = self.predecessor {
            digest(&d)?;
        }
        Ok(())
    }
}

/// One immutable destination-bound operation, including its expected head.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Globally retained operation ID, not reusable for another payload.
    pub operation: String,
    /// Exact expected current authority, including predecessor and scope.
    pub source: Authority,
    /// One destination; another placement needs another admitted handoff.
    pub destination: Placement,
    /// Must equal source generation plus one without overflow.
    pub successor_generation: u64,
    /// Existing admitted signer/verifier profile revision.
    pub profile: String,
}

impl Request {
    /// Check the tuple, destination and exact successor relationship.
    pub fn validate(&self) -> ModelResult<()> {
        identifier(&self.operation)?;
        identifier(&self.profile)?;
        self.source.validate()?;
        self.destination.validate()?;
        require(
            self.destination != self.source.owner,
            "destination equals source",
        )?;
        require(
            self.source.generation.checked_add(1) == Some(self.successor_generation),
            "skipped or exhausted generation",
        )
    }

    /// Canonical typed content identity, independent of input JSON key order.
    pub fn digest(&self) -> ModelResult<Digest> {
        self.validate()?;
        hash(b"quipu-ownership-model/request/v1\0", self)
    }
}

/// A sealed cut's coordinates and digests; actual bytes are checked by adapters.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cut {
    /// Last transaction in the source epoch (zero permits an empty store).
    pub transaction: u64,
    /// Exact canonical fact-cut digest.
    pub facts: Digest,
    /// Exact full artifact digest, not a scoped replica digest.
    pub artifact: Digest,
}

/// Candidate retirement body. This struct contains NO signature or trust grant.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Certificate {
    /// Immutable source/destination/operation/profile binding.
    pub request: Request,
    /// Immutable sealed source cut and full artifact.
    pub cut: Cut,
}

impl Certificate {
    /// Check content shape; authentication remains a separate prerequisite.
    pub fn validate(&self) -> ModelResult<()> {
        self.request.validate()?;
        digest(&self.cut.facts)?;
        digest(&self.cut.artifact)
    }

    /// Domain-separated candidate certificate identity.
    pub fn digest(&self) -> ModelResult<Digest> {
        self.validate()?;
        hash(b"quipu-ownership-model/retirement/v1\0", self)
    }

    /// Derive the only possible successor tuple for a proposed fresh epoch.
    pub fn successor(&self, epoch: &str) -> ModelResult<Authority> {
        self.validate()?;
        identifier(epoch)?;
        require(epoch != self.request.source.epoch, "reused source epoch")?;
        Ok(Authority {
            store_id: self.request.source.store_id.clone(),
            unit: self.request.source.unit.clone(),
            generation: self.request.successor_generation,
            epoch: epoch.into(),
            owner: self.request.destination.clone(),
            policy: self.request.source.policy.clone(),
            predecessor: Some(self.digest()?),
        })
    }
}

/// Durable idempotent activation outcome; retrieving it does not restart a writer.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Activation {
    /// Complete immutable retirement body, not only an unauthenticated ID.
    pub certificate: Certificate,
    /// Exact admitted successor tuple and epoch.
    pub authority: Authority,
}

/// Candidate protected-journal states. Missing state is never legacy INACTIVE.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum Journal {
    /// May admit local commits only with agreeing data and process admission.
    Active {
        /// Exact admitted tuple.
        authority: Authority,
    },
    /// Irreversible local barrier; no usable retirement receipt yet.
    Retiring {
        /// Reserved immutable operation.
        request: Request,
    },
    /// Completed retirement; source cannot become active by recovery.
    Retired {
        /// Durable cut/outbox and journal agree on this body.
        certificate: Certificate,
    },
    /// Destination install is incomplete or admission has not finished.
    Pending {
        /// Expected source retirement.
        certificate: Certificate,
        /// Only proposed successor.
        successor: Authority,
    },
}

/// Candidate record payloads, not a runtime transport or on-disk migration.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum Payload {
    /// Protected role record.
    Journal {
        /// Candidate role.
        journal: Journal,
    },
    /// Reserved request/idempotency ledger entry.
    Operation {
        /// Immutable operation.
        request: Request,
    },
    /// Retained source cut/outbox record.
    Retirement {
        /// Candidate completed retirement.
        certificate: Certificate,
    },
    /// Durable destination result and receipt consumption.
    Activation {
        /// Idempotent result.
        activation: Activation,
    },
    /// Fresh startup response body; authentication is not implemented here.
    Startup {
        /// Exact externally current head.
        authority: Authority,
        /// Nonce bound to the current challenge.
        nonce: String,
    },
}

/// Strict, bounded record envelope. Decoding never starts a writer.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// Must equal RECORD_VERSION; unknown versions refuse.
    pub version: u16,
    /// Explicit typed payload with no permissive legacy fallback.
    pub payload: Payload,
}

impl Record {
    /// Validate a candidate's internal relationships, not external truth.
    pub fn validate(&self) -> ModelResult<()> {
        require(self.version == RECORD_VERSION, "unknown record version")?;
        match &self.payload {
            Payload::Operation { request } => request.validate(),
            Payload::Retirement { certificate } => certificate.validate(),
            Payload::Startup { authority, nonce } => {
                authority.validate()?;
                identifier(nonce)
            }
            Payload::Activation { activation } => {
                activation.authority.validate()?;
                require(
                    activation
                        .certificate
                        .successor(&activation.authority.epoch)?
                        == activation.authority,
                    "activation tuple disagrees with retirement",
                )
            }
            Payload::Journal { journal } => match journal {
                Journal::Active { authority } => authority.validate(),
                Journal::Retiring { request } => request.validate(),
                Journal::Retired { certificate } => certificate.validate(),
                Journal::Pending {
                    certificate,
                    successor,
                } => {
                    successor.validate()?;
                    require(
                        certificate.successor(&successor.epoch)? == *successor,
                        "pending tuple disagrees",
                    )
                }
            },
        }
    }

    /// Encode deterministic typed JSON after validation.
    pub fn encode(&self) -> ModelResult<Vec<u8>> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        require(bytes.len() <= MAX_RECORD_BYTES, "oversized record")?;
        Ok(bytes)
    }

    /// Decode strictly, including unknown/duplicate fields and nested relations.
    pub fn decode(bytes: &[u8]) -> ModelResult<Self> {
        require(bytes.len() <= MAX_RECORD_BYTES, "oversized record")?;
        let record: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        record.validate()?;
        Ok(record)
    }
}
