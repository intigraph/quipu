//! Attested writes: an ordinary governed write that is admitted only when an
//! enrolled human signer approved its exact content on their own device.
//!
//! Default-off and domain-free. The operator enrolls signers over the store
//! file and marks the graphs that accept attested writes only. A caller asks
//! for a challenge naming a signer, a graph, the datums and any live
//! preconditions; the server stores the exact canonical bytes and returns
//! them to sign. The commit carries only the nonce and the assertion, and
//! runs one savepoint: verify, check preconditions, run the normal graph
//! gates, consume the challenge, write, and record a receipt. A refusal
//! records nothing. A lost response is recovered by the receipt, never by a
//! second commit.
//! # arming: library — callers invoke the native methods; no transport is armed.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{Datum, Store};
use crate::error::{Error, Result};
use crate::types::{Op, Value};
use crate::verdict_schemes::{Scheme, webauthn};

mod admission;
mod expect;
mod operator;
#[cfg(test)]
mod tests;
mod transaction;

/// Signed-message domain; never interpretable as any other signed object.
pub const DOMAIN: &str = "quipu-attested-write-v1";

/// The `source` recorded on every attested write.
pub const SOURCE: &str = "attested-write";

const MAX_DATUMS: usize = 64;
const MAX_TEXT_BYTES: usize = 4096;
const MAX_EXPECT: usize = 16;
const MAX_MESSAGE_BYTES: usize = 32768;
const CHALLENGE_SECS: i64 = 300;

/// One enrolled human signer. Public key material only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Signer {
    /// Unpadded base64url authenticator credential ID.
    pub id: String,
    /// `WebAuthn` scheme tag.
    pub scheme: String,
    /// COSE public key bytes.
    pub cose_key: Vec<u8>,
    /// Exact dedicated HTTPS origin, without a path.
    pub origin: String,
    /// Relying-party host of that origin.
    pub rp_id: String,
    /// The independently established human identity this signer stands for.
    pub actor: String,
    /// The only graphs this signer may write.
    pub graphs: Vec<String>,
    /// Binding deadline, epoch seconds.
    pub expires_at: i64,
}

/// An operator's view of one signer.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignerStatus {
    /// The enrolled signer.
    pub signer: Signer,
    /// Short fingerprint of the COSE key, for comparing with the device.
    pub fingerprint: String,
    /// Permanently revoked.
    pub revoked: bool,
    /// Last recorded authenticator counter.
    pub sign_count: u32,
}

/// A value in an attested datum or expectation, typed so that a string never
/// matches an IRI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum TypedValue {
    /// An integer literal.
    Int(i64),
    /// A plain string literal.
    Str(String),
    /// A reference to an IRI.
    Iri(String),
}

/// One triple to assert, with server-assigned valid time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttestedDatum {
    /// Subject IRI.
    pub s: String,
    /// Predicate IRI.
    pub p: String,
    /// Object.
    pub o: TypedValue,
}

/// One live precondition: the single current value of `(s, p)` in `graph`,
/// or no value when `o` is `None`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Named graph that holds the slot.
    pub graph: String,
    /// Subject IRI.
    pub s: String,
    /// Predicate IRI.
    pub p: String,
    /// Expected current value; `None` expects no asserted value.
    pub o: Option<TypedValue>,
}

/// What a caller proposes for signature.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    /// Enrolled signer ID.
    pub signer: String,
    /// Target graph, which must be attested-only and on the signer's list.
    pub graph: String,
    /// 1 to 64 triples to assert.
    pub datums: Vec<AttestedDatum>,
    /// Up to 16 live preconditions, checked again at commit.
    #[serde(default)]
    pub expect: Vec<Expect>,
}

/// The complete server-issued content that is signed. Serialize this struct,
/// never caller JSON; the commit re-derives these bytes from storage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    /// Always [`DOMAIN`].
    pub domain: String,
    /// Receiving store lineage.
    pub audience: String,
    /// Random 256-bit one-use identifier.
    pub nonce: String,
    /// The signer's origin.
    pub origin: String,
    /// The signer's relying party.
    pub rp_id: String,
    /// The signer's human identity.
    pub actor: String,
    /// The proposal, exactly as stored.
    pub proposal: Proposal,
    /// Server-clock deadline, at most five minutes away.
    pub expires_at: i64,
}

impl Challenge {
    /// The exact signed message.
    pub fn message(&self) -> Result<Vec<u8>> {
        encode(self)
    }

    /// Raw challenge for `navigator.credentials.get()`.
    pub fn webauthn_challenge(&self) -> Result<[u8; 32]> {
        Ok(crate::verdict_schemes::webauthn_challenge(&self.message()?))
    }
}

/// Retained audit material for one admitted write.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Receipt {
    /// The signed challenge.
    pub challenge: Challenge,
    /// `sha256:` of the signed message.
    pub content_hash: String,
    /// Enrolled public key snapshot, for later re-verification.
    pub cose_key: Vec<u8>,
    /// Scheme used.
    pub scheme: String,
    /// Raw assertion bytes, retained exactly.
    pub authenticator_data: Vec<u8>,
    /// Authenticator-signed client data.
    pub client_data_json: Vec<u8>,
    /// Signature.
    pub signature: Vec<u8>,
    /// Server time of admission, epoch seconds.
    pub recorded_at: i64,
}

fn refuse(message: &str) -> Error {
    Error::PolicyDenied(format!("attested write: {message}"))
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| Error::InvalidValue(e.to_string()))
}

fn decode<T: for<'a> Deserialize<'a>>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|e| Error::InvalidValue(e.to_string()))
}

fn now() -> Result<i64> {
    i64::try_from(crate::time::epoch_secs()).map_err(|_| refuse("invalid server clock"))
}

fn iri(text: &str) -> bool {
    text.len() <= MAX_TEXT_BYTES && oxrdf::NamedNode::new(text).is_ok()
}

fn valid_value(value: &TypedValue) -> bool {
    match value {
        TypedValue::Int(_) => true,
        TypedValue::Str(s) => s.len() <= MAX_TEXT_BYTES,
        TypedValue::Iri(i) => iri(i),
    }
}

/// Short operator-readable fingerprint of a COSE public key: the first 16 hex
/// digits of its SHA-256, in groups of four, for comparing with the device
/// during enrollment. It names a key; it is not a security proof.
pub fn key_fingerprint(cose_key: &[u8]) -> String {
    let digest = hex::encode(ring::digest::digest(&ring::digest::SHA256, cose_key));
    digest.as_bytes()[..16]
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("-")
}
