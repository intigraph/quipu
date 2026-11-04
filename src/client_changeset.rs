//! Signed offline proposals. This is admission, never upstream replication.
//!
//! The portable envelope and overlay build for native and Wasm. Master-side
//! registration, three-way merge and atomic receipts live in `store`.
//! # arming: library — no HTTP route or credential enrollment is armed.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::changeset::{ChangeSet, Header, SPARQL_UPDATE, Statement};
use crate::{Error, Result};

/// The signing domain, distinct from verdicts and replication frames.
pub const DOMAIN: &str = "quipu-changeset-v1";
const MAX_BODY: usize = 1 << 20;

pub(crate) fn refuse(reason: &str) -> Error {
    Error::CannotVerify(format!("{DOMAIN}: {reason}"))
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn field(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}

fn hash(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|v| {
        v.len() == 64
            && v.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    })
}

fn operation_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        })
        && value
            .bytes()
            .any(|c| (b'1'..=b'9').contains(&c) || (b'a'..=b'f').contains(&c))
}

/// A frozen base row, including its valid interval. Dictionary IDs never cross
/// the boundary. Blank nodes must be skolemized before opening an overlay.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaseRow {
    /// Canonical N-Triples terms, without a trailing dot.
    pub triple: String,
    /// Inclusive valid-time start, normalized to UTC seconds.
    pub valid_from: String,
    /// Exclusive valid-time end; None means open-ended.
    pub valid_to: Option<String>,
}

impl BaseRow {
    fn validate(&self) -> Result<()> {
        let statement = Statement::new(&self.triple, None);
        let quad = statement.to_quad()?;
        if statement.has_blank_node() || Statement::from_quad(&quad).triple != self.triple {
            return Err(refuse("base row must be canonical and skolemized"));
        }
        let normalized = crate::time::normalize_rfc3339_utc(&self.valid_from);
        if normalized.as_deref() != Some(self.valid_from.as_str()) {
            return Err(refuse("base valid_from is not canonical UTC"));
        }
        if let Some(to) = &self.valid_to
            && (crate::time::normalize_rfc3339_utc(to).as_deref() != Some(to.as_str())
                || to <= &self.valid_from)
        {
            return Err(refuse("base valid interval is not canonical or is empty"));
        }
        Ok(())
    }
}

/// Source-bound immutable journal. Loading it never retargets it to a new
/// source, epoch or transaction. The master independently reconstructs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaseJournal {
    pub(crate) store_id: String,
    pub(crate) log_epoch: String,
    pub(crate) base_tx: i64,
    pub(crate) graph: String,
    pub(crate) shapes_hash: String,
    rows: BTreeSet<BaseRow>,
}

impl BaseJournal {
    /// Construct a journal from an independently acquired source slice.
    pub fn new(
        store_id: String,
        log_epoch: String,
        base_tx: i64,
        graph: String,
        shapes_hash: String,
        rows: impl IntoIterator<Item = BaseRow>,
    ) -> Result<Self> {
        let journal = Self {
            store_id,
            log_epoch,
            base_tx,
            graph,
            shapes_hash,
            rows: rows.into_iter().collect(),
        };
        journal.validate()?;
        Ok(journal)
    }

    /// Check the canonical binding, rows and valid intervals.
    pub fn validate(&self) -> Result<()> {
        if !field(&self.store_id)
            || !field(&self.log_epoch)
            || self.base_tx < 1
            || !hash(&self.shapes_hash)
        {
            return Err(refuse("invalid base binding"));
        }
        if self.graph != "ROOT" {
            oxrdf::NamedNode::new(self.graph.clone()).map_err(|_| refuse("invalid graph IRI"))?;
        }
        for row in &self.rows {
            row.validate()?;
        }
        Ok(())
    }

    /// Frozen canonical rows; no mutable view is exposed.
    pub fn rows(&self) -> &BTreeSet<BaseRow> {
        &self.rows
    }
    /// Source lineage retained by this base.
    pub fn store_id(&self) -> &str {
        &self.store_id
    }
    /// Source epoch retained by this base.
    pub fn log_epoch(&self) -> &str {
        &self.log_epoch
    }
    /// Source transaction retained by this base.
    pub fn base_tx(&self) -> i64 {
        self.base_tx
    }
    /// Explicit graph covered by this journal.
    pub fn graph(&self) -> &str {
        &self.graph
    }

    /// With no blank nodes, sorted canonical N-Quads plus intervals are an
    /// unambiguous RDFC representation. JSON strings escape literal newlines.
    pub fn slice_hash(&self) -> Result<String> {
        self.validate()?;
        let rows: Vec<_> = self
            .rows
            .iter()
            .map(|row| {
                let graph = (self.graph != "ROOT").then(|| self.graph.clone());
                (
                    format!("{} .", Statement::new(&row.triple, graph).nquad()),
                    &row.valid_from,
                    &row.valid_to,
                )
            })
            .collect();
        Ok(digest(
            &serde_json::to_vec(&rows).map_err(|e| Error::Serialization(e.to_string()))?,
        ))
    }

    fn statements(&self) -> BTreeSet<Statement> {
        self.rows
            .iter()
            .map(|row| {
                Statement::new(
                    &row.triple,
                    (self.graph != "ROOT").then(|| self.graph.clone()),
                )
            })
            .collect()
    }
}

/// Canonical signed fields. Every string is newline-free before rendering;
/// `=` inside a value cannot introduce a new field. Body hashing additionally
/// binds all unsigned-format headers, including future header extensions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// Receiving master lineage.
    pub audience: String,
    /// Store lineage that owns the frozen base.
    pub store_id: String,
    /// Source epoch; never retargeted after restore.
    pub log_epoch: String,
    /// Persistent local overlay identity; distinct from source and operation.
    pub fork_id: String,
    /// One explicit writable graph; ROOT names the default graph.
    pub graph: String,
    /// The same scope digest used by the cut and replication stream.
    pub scope_digest: String,
    /// Source transaction at which the journal was frozen.
    pub base_tx: i64,
    /// Canonical base quads and valid intervals.
    pub base_slice_hash: String,
    /// Shape registry the client used; current shapes must still match.
    pub shapes_hash: String,
    /// The unsigned `ChangeSet` statement digest, prefixed once.
    pub delta_sha256: String,
    /// Exact unsigned body bytes, including all headers.
    pub body_sha256: String,
    /// Governed registration IRI holding the signing key.
    pub key_id: String,
    /// Verifier name on that same registration.
    pub signer: String,
    /// Non-nil lowercase UUID retained for identical retries.
    pub operation_id: String,
    /// Claimed signing time; never used to backdate authority.
    pub issued_at: String,
    /// 128-bit lowercase hex value, included in the signature.
    pub nonce: String,
}

impl Envelope {
    /// Validate fields and render the canonical bytes to sign.
    pub fn message(&self) -> Result<Vec<u8>> {
        let fields = [
            ("audience", self.audience.as_str()),
            ("store_id", &self.store_id),
            ("log_epoch", &self.log_epoch),
            ("fork_id", &self.fork_id),
            ("graph", &self.graph),
            ("scope_digest", &self.scope_digest),
            ("base_slice_hash", &self.base_slice_hash),
            ("shapes_hash", &self.shapes_hash),
            ("delta_sha256", &self.delta_sha256),
            ("body_sha256", &self.body_sha256),
            ("key_id", &self.key_id),
            ("signer", &self.signer),
            ("operation_id", &self.operation_id),
            ("issued_at", &self.issued_at),
            ("nonce", &self.nonce),
        ];
        if fields.iter().any(|(_, value)| !field(value))
            || oxrdf::NamedNode::new(self.fork_id.clone()).is_err()
            || self.base_tx < 1
            || !operation_id(&self.operation_id)
            || self.nonce.len() != 32
            || !self
                .nonce
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || crate::time::normalize_rfc3339_utc(&self.issued_at).as_deref()
                != Some(self.issued_at.as_str())
            || [
                &self.scope_digest,
                &self.base_slice_hash,
                &self.shapes_hash,
                &self.delta_sha256,
                &self.body_sha256,
            ]
            .into_iter()
            .any(|s| !hash(s))
        {
            return Err(refuse("invalid envelope field"));
        }
        let mut out = format!("{DOMAIN}\nrole=client\nbase_tx={}\n", self.base_tx);
        for (key, value) in fields {
            out.push_str(&format!("{key}={value}\n"));
        }
        Ok(out.into_bytes())
    }
}

/// The wire object. No private key material is serialized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedChangeSet {
    /// Canonical domain-separated source and authority binding.
    pub envelope: Envelope,
    /// Store-wide quipu-changeset v1 SPARQL Update body.
    pub body: String,
    /// Ed25519 signature in lowercase hex.
    pub signature: String,
}

impl SignedChangeSet {
    /// Validate the body against every signed binding.
    pub fn changeset(&self) -> Result<ChangeSet> {
        validate_body(&self.envelope, &self.body)
    }

    /// Verify both body bindings and the shared-crate signature.
    pub fn verify(&self, public_key: &str) -> Result<()> {
        self.changeset()?;
        if !quipu_signing::verify_hex(public_key, &self.envelope.message()?, &self.signature) {
            return Err(refuse("invalid signature"));
        }
        Ok(())
    }

    /// Fingerprint every signed binding for operation-id idempotency.
    pub fn fingerprint(&self) -> Result<String> {
        Ok(digest(&self.envelope.message()?))
    }
}

mod overlay;
pub use overlay::{ClientOverlay, PreparedChangeSet, ProposalIdentity};

#[cfg(test)]
#[path = "client_changeset_tests.rs"]
mod tests;

fn validate_body(envelope: &Envelope, body: &str) -> Result<ChangeSet> {
    envelope.message()?;
    if body.len() > MAX_BODY || digest(body.as_bytes()) != envelope.body_sha256 {
        return Err(refuse("body hash or size mismatch"));
    }
    let cs = ChangeSet::parse(body, SPARQL_UPDATE)?;
    if format!("sha256:{}", cs.digest()) != envelope.delta_sha256
        || cs.header.base_tx != Some(envelope.base_tx)
        || cs.header.store_id.as_deref() != Some(envelope.store_id.as_str())
        || cs.header.shapes_hash.as_deref() != Some(envelope.shapes_hash.as_str())
        || cs.header.target_tx.is_some()
        || cs.header.base_share.is_some()
        || cs.header.valid_time.is_some()
    {
        return Err(refuse(
            "unsigned header disagrees with the signed binding or is unsupported",
        ));
    }
    for s in cs.deletes.iter().chain(&cs.inserts) {
        let expected = (envelope.graph != "ROOT").then_some(envelope.graph.as_str());
        if s.graph.as_deref() != expected || s.has_blank_node() {
            return Err(refuse("out-of-scope graph or blank node"));
        }
        s.to_quad()?;
    }
    Ok(cs)
}
