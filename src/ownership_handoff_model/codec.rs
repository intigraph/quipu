use super::records::identifier;
use super::*;
use serde::{Deserialize, Serialize};

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
        /// Full externally current head, including reservation/retirement phase.
        head: WitnessHead,
        /// Nonce bound to the current challenge.
        nonce: String,
    },
    /// External witness phase/head record; a bare tuple cannot replace it.
    Witness {
        /// Complete phase-bound head.
        head: WitnessHead,
    },
}

/// Strict, bounded record envelope. Decoding never starts a writer.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// Must equal `RECORD_VERSION`; unknown versions refuse.
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
            Payload::Startup { head, nonce } => {
                head.validate()?;
                identifier(nonce)
            }
            Payload::Witness { head } => head.validate(),
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
