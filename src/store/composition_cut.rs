//! Versioned composition-cut tokens (aegis-n0wc.5 B4; design:
//! `docs/design/per-graph-writable-stores.md`, "Local transactions and
//! multi-file cuts").
//!
//! A transaction coordinate is `(store_id, log_epoch, local_tx)` and is only
//! meaningful inside its own file. No scalar is compared across files. A cut
//! names every selected graph with its file's coordinate, owner generation and
//! policy revision, plus the catalog revision; its canonical digest is the
//! cursor. A cursor from a different store or epoch REFUSES; it never answers
//! from another transaction sequence.
//!
//! This module is source only and default-off: nothing serves a cut yet. It
//! admits a cut against the reads a strict capture just produced. Historical
//! evaluation at a cut (vector mode) is separate work.

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::graph_owner::CapturedRead;
use crate::{Error, Result};

/// Token format version. A new layout gets a new number; old numbers are
/// never reinterpreted.
pub const CUT_VERSION: u32 = 1;
const PREFIX: &str = "qcut1.";

fn refused(kind: &str, detail: impl std::fmt::Display) -> Error {
    Error::Store(format!("composition cut refused ({kind}): {detail}"))
}

/// One selected graph's position: its file's coordinate and the owner pins it
/// was captured under.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutMember {
    /// Durable lineage of the file holding the graph.
    pub store_id: String,
    /// That file's change-log epoch.
    pub log_epoch: String,
    /// That file's committed transaction head at the cut.
    pub local_tx: i64,
    /// Owner generation of the graph's route.
    pub generation: u64,
    /// Policy revision installed on the graph's owner.
    pub policy: String,
}

/// A strict multi-file cut over an explicit graph selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionCut {
    version: u32,
    catalog_revision: u64,
    members: BTreeMap<String, CutMember>,
}

impl CompositionCut {
    /// The cut a strict capture produced: one member per selected graph.
    pub fn capture(reads: &[CapturedRead<'_>]) -> Result<Self> {
        let first = reads
            .first()
            .ok_or_else(|| refused("empty", "no selected graph"))?;
        let mut members = BTreeMap::new();
        for read in reads {
            if read.catalog_revision != first.catalog_revision {
                return Err(refused(
                    "catalog-changed",
                    "capture spans catalog revisions",
                ));
            }
            let member = CutMember {
                store_id: read.route.store_id.clone(),
                log_epoch: read.route.log_epoch.clone(),
                local_tx: read.local_tx,
                generation: read.route.generation,
                policy: read.route.policy.clone(),
            };
            if members.insert(read.route.graph.clone(), member).is_some() {
                return Err(refused("malformed", "duplicate selected graph"));
            }
        }
        Ok(Self {
            version: CUT_VERSION,
            catalog_revision: first.catalog_revision,
            members,
        })
    }

    /// Token format version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Catalog revision the cut was captured under.
    pub fn catalog_revision(&self) -> u64 {
        self.catalog_revision
    }

    /// Selected graphs, in canonical (sorted IRI) order.
    pub fn members(&self) -> &BTreeMap<String, CutMember> {
        &self.members
    }

    /// Canonical bytes: JSON with every object's keys sorted, no whitespace.
    fn canonical(&self) -> Vec<u8> {
        let value = serde_json::to_value(self).expect("a cut always serializes");
        serde_json::to_vec(&value).expect("a JSON value always serializes")
    }

    /// The cursor: sha256 (hex) of the canonical bytes.
    pub fn digest(&self) -> String {
        hex::encode(Sha256::digest(self.canonical()))
    }

    /// `qcut1.` + base64url (no padding) of the canonical bytes.
    pub fn to_token(&self) -> String {
        format!(
            "{PREFIX}{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.canonical())
        )
    }

    /// Parse a token. Refuses another version, unknown or missing fields, an
    /// empty selection, a negative head and any non-canonical encoding.
    pub fn from_token(token: &str) -> Result<Self> {
        let body = token
            .strip_prefix(PREFIX)
            .ok_or_else(|| refused("unsupported-version", "token does not start with qcut1."))?;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(body)
            .map_err(|e| refused("malformed", e))?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| refused("malformed", e))?;
        if value.get("version").and_then(serde_json::Value::as_u64) != Some(CUT_VERSION.into()) {
            return Err(refused("unsupported-version", "body version is not 1"));
        }
        let cut: Self = serde_json::from_value(value).map_err(|e| refused("malformed", e))?;
        if cut.members.is_empty() {
            return Err(refused("empty", "a cut selects at least one graph"));
        }
        if cut.members.values().any(|m| m.local_tx < 0) {
            return Err(refused("malformed", "negative local_tx"));
        }
        if cut.canonical() != bytes {
            return Err(refused("non-canonical", "one cut has exactly one token"));
        }
        Ok(cut)
    }

    /// Admit this cut for the reads a strict capture of the SAME selection
    /// just produced. Every member must be the same file lineage and epoch,
    /// the same owner pins and catalog, and no later than the file's head.
    pub fn admit(&self, reads: &[CapturedRead<'_>]) -> Result<()> {
        if self.version != CUT_VERSION {
            return Err(refused("unsupported-version", self.version));
        }
        let selected: BTreeSet<&str> = reads.iter().map(|r| r.route.graph.as_str()).collect();
        if let Some(extra) = self.members.keys().find(|g| !selected.contains(g.as_str())) {
            return Err(refused("extra-member", extra));
        }
        for read in reads {
            let graph = &read.route.graph;
            let member = self
                .members
                .get(graph)
                .ok_or_else(|| refused("missing-member", graph))?;
            check_coordinate(&member.store_id, &member.log_epoch, member.local_tx, read)?;
            if member.generation != read.route.generation {
                return Err(refused("generation-changed", graph));
            }
            if member.policy != read.route.policy {
                return Err(refused("policy-changed", graph));
            }
            if self.catalog_revision != read.catalog_revision {
                return Err(refused("catalog-changed", graph));
            }
        }
        Ok(())
    }
}

/// The legacy transaction scalar (`as_of_tx`, Seeds `--at N`), explicitly
/// bound to the one file it was read from. It keeps its exact meaning there and
/// refuses on any selection that spans files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundScalar {
    /// File lineage the scalar belongs to.
    pub store_id: String,
    /// That file's change-log epoch.
    pub log_epoch: String,
    /// The transaction id in that file.
    pub local_tx: i64,
}

impl BoundScalar {
    /// Admit the scalar for `reads`: every selected graph must live in the
    /// scalar's own file and epoch, at or after its transaction.
    pub fn admit(&self, reads: &[CapturedRead<'_>]) -> Result<()> {
        let files: BTreeSet<&str> = reads.iter().map(|r| r.route.store_id.as_str()).collect();
        if files.len() > 1 {
            return Err(refused(
                "scalar-spans-files",
                format!(
                    "the selection spans {} files; a transaction id is file-local, \
                     so request a composition cut",
                    files.len()
                ),
            ));
        }
        let read = reads
            .first()
            .ok_or_else(|| refused("empty", "no selected graph"))?;
        check_coordinate(&self.store_id, &self.log_epoch, self.local_tx, read)
    }
}

fn check_coordinate(
    store_id: &str,
    log_epoch: &str,
    local_tx: i64,
    read: &CapturedRead<'_>,
) -> Result<()> {
    let graph = &read.route.graph;
    if store_id != read.route.store_id {
        return Err(refused("foreign-store", graph));
    }
    if log_epoch != read.route.log_epoch {
        return Err(refused("foreign-epoch", graph));
    }
    if local_tx > read.local_tx {
        return Err(refused(
            "ahead-of-head",
            format!("{graph}: cut {local_tx} > head {}", read.local_tx),
        ));
    }
    Ok(())
}
