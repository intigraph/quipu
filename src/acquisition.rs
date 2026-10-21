//! Complete, bounded graph acquisition from one source read transaction.
//!
//! The receipt describes the source, never a later artifact's local store.

use std::collections::{BTreeMap, BTreeSet};

use oxrdf::{BlankNode, NamedNode, NamedOrBlankNode, Triple};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{Error, Result, Store};

/// Versioned, qualified-main acquisition with a fenced attachment census.
pub mod composed;

/// Metadata-only eligibility observation; never an acquisition receipt.
#[path = "acquisition/eligibility.rs"]
pub mod eligibility;

#[path = "acquisition/census.rs"]
mod census;
#[path = "acquisition/main_graph.rs"]
mod main_graph;
#[path = "acquisition/provider_fence.rs"]
mod provider_fence;

/// Fixed response ceiling, shared with the payload-returning share endpoint.
pub const MAX_BYTES: usize = crate::share::SHARE_PAYLOAD_MAX_BYTES;

/// Exact source selection; no dataset expansion, arbitrary queries, or paging.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Source lineage expected by the caller; a mismatch is a refusal.
    pub source_store_id: String,
    /// Exact registered named graph IRIs, including any comment graph.
    pub graphs: Vec<String>,
    /// Optional transaction pin; absent means one captured present snapshot.
    #[serde(default, deserialize_with = "strict_tx")]
    pub tx: Option<i64>,
    /// Caller may lower, never raise, the response cap.
    pub max_bytes: Option<usize>,
}

/// Hashes and sizes of each complete source graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphReceipt {
    /// Number of triples in the canonical graph.
    pub triples: usize,
    /// SHA-256 of its exact canonical N-Triples bytes.
    pub sha256: String,
}

/// Source authority and pin application, separate from artifact metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    /// Contract version; consumers must recognize it explicitly.
    pub schema: String,
    /// Persistent source lineage, not an endpoint or a scratch store ID.
    pub source_store_id: String,
    /// Committed source head observed inside the coherent read transaction.
    pub source_head_tx: i64,
    /// Caller-requested pin, distinct from the pin actually applied.
    #[serde(deserialize_with = "nullable_tx")]
    pub requested_tx: Option<i64>,
    /// The file-local transaction applied to every graph.
    pub applied_tx: i64,
    /// This contract's supported history model.
    pub history: String,
    /// Oldest transaction retained in this source's transaction log.
    pub history_min_tx: i64,
    /// Stable triple ordering while preserving source blank-node identities.
    pub canonicalization: String,
    /// Compiled producer version.
    pub producer_version: String,
    /// Compiled source commit.
    pub producer_git_sha: String,
    /// Whether the producer was built from modified source.
    pub producer_git_dirty: bool,
    /// Exact graph scope and content identities.
    pub graphs: BTreeMap<String, GraphReceipt>,
    /// True only after all reads and response-cap checks succeed.
    pub complete: bool,
}

/// Entire acquisition; receipt hash is independent of artifact-local metadata.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    /// Source receipt.
    pub source_receipt: Receipt,
    /// Hash of compact UTF-8 JSON with recursively sorted object keys.
    pub source_receipt_sha256: String,
    /// Exact canonical N-Triples graph bytes as UTF-8 strings.
    pub graphs: BTreeMap<String, String>,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidValue(message.into())
}

/// Compact UTF-8 JSON with sorted object keys and no trailing newline.
/// Uses the deterministic JSON map encoding of the share manifest contract.
pub fn receipt_bytes(receipt: &Receipt) -> Result<Vec<u8>> {
    let value = serde_json::to_value(receipt).map_err(|e| invalid(e.to_string()))?;
    serde_json::to_vec(&value).map_err(|e| invalid(e.to_string()))
}

fn strict_tx<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<i64>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    value
        .as_i64()
        .filter(|tx| *tx > 0)
        .map(Some)
        .ok_or_else(|| serde::de::Error::custom("tx must be a positive signed 64-bit JSON integer"))
}

fn nullable_tx<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<i64>, D::Error> {
    Option::<i64>::deserialize(deserializer)
}

/// Acquire all selected graphs atomically; errors produce no partial receipt.
pub fn acquire(store: &Store, request: &Request) -> Result<Payload> {
    acquire_with_control(store, request, || Ok(()))
}

fn acquire_with_control(
    store: &Store,
    request: &Request,
    after_head: impl FnOnce() -> Result<()>,
) -> Result<Payload> {
    let started = crate::time::Stopwatch::start();
    let check_deadline = || {
        if let Some(deadline) =
            crate::time::request_deadline().filter(crate::time::Deadline::passed)
        {
            return Err(Error::QueryTimeout {
                elapsed_ms: started.elapsed_ms(),
                limit_ms: deadline.millis_from(&started),
            });
        }
        Ok(())
    };
    check_deadline()?;
    // The initial contract refuses attached stores even for present reads: a
    // single main-file head cannot attest another file's mutable generation.
    if store.has_attachments() {
        return Err(invalid("acquisition/v1 does not support attached stores"));
    }
    if request.graphs.is_empty() || request.graphs.len() > 16 {
        return Err(invalid("acquisition requires 1..=16 explicit named graphs"));
    }
    let read = store.conn.unchecked_transaction()?;
    let source_store_id = store.store_id()?;
    if source_store_id != request.source_store_id {
        return Err(invalid("acquisition source store identity mismatch"));
    }
    let head = store.latest_tx_id()?;
    after_head()?;
    let mut pin = json!({});
    if let Some(tx) = request.tx {
        pin["tx"] = json!(tx);
    }
    let requested_tx = crate::read_pin::requested_tx(store, &pin)?;
    let applied_tx = requested_tx.unwrap_or(head);
    if applied_tx == 0 {
        return Err(invalid("acquisition source has no committed transaction"));
    }
    let mut graphs = BTreeMap::new();
    let mut receipts = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for iri in &request.graphs {
        check_deadline()?;
        NamedNode::new(iri.clone()).map_err(|e| invalid(format!("graph IRI: {e}")))?;
        let graph = store.registered_graph_id(iri)?;
        if !seen.insert(iri) || graph.is_none() || store.is_dataset(iri)? {
            return Err(invalid(
                "duplicate, unregistered, or dataset-alias acquisition graph",
            ));
        }
        if store
            .graph_class(graph.expect("registered graph checked"))?
            .as_deref()
            != Some("committed")
        {
            return Err(invalid(
                "acquisition requires a committed graph, not an overlay or fork",
            ));
        }
        store.check_label_floor(&[graph.expect("registered graph checked")])?;
        // Generated fixed query: graph cannot be overridden by caller FROM,
        // SERVICE, entailment, a filter, or a second graph clock.
        let input = json!({
            "query": "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }",
            "graph": iri, "tx": applied_tx,
        });
        let (result, truncated) = crate::mcp::query_result(store, &input)?;
        if truncated {
            return Err(invalid("acquisition graph truncated; completeness UNKNOWN"));
        }
        let crate::QueryResult::Graph(triples) = result else {
            return Err(invalid("acquisition graph result unavailable"));
        };
        let triples: Result<Vec<_>> = triples
            .into_iter()
            .map(|t| {
                let subject = if let Some(label) = t.subject.strip_prefix(crate::rdf::BLANK_PREFIX)
                {
                    NamedOrBlankNode::BlankNode(
                        BlankNode::new(label).map_err(|e| invalid(e.to_string()))?,
                    )
                } else {
                    NamedOrBlankNode::NamedNode(
                        NamedNode::new(t.subject).map_err(|e| invalid(e.to_string()))?,
                    )
                };
                Ok(Triple {
                    subject,
                    predicate: NamedNode::new(t.predicate).map_err(|e| invalid(e.to_string()))?,
                    object: crate::rdf::value_to_term(store, &t.object)?,
                })
            })
            .collect();
        let triples = triples?;
        let bytes =
            crate::rdf_export::serialize_triples_canonical(&triples, oxrdfio::RdfFormat::NTriples)?;
        receipts.insert(
            iri.clone(),
            GraphReceipt {
                triples: bytes.iter().filter(|byte| **byte == b'\n').count(),
                sha256: crate::share::sha256(&bytes),
            },
        );
        graphs.insert(
            iri.clone(),
            String::from_utf8(bytes).map_err(|e| invalid(e.to_string()))?,
        );
    }
    let source_receipt = Receipt {
        schema: "quipu-acquisition/v1".into(),
        source_store_id,
        source_head_tx: head,
        requested_tx,
        applied_tx,
        history: "file-local-bitemporal-eavt".into(),
        history_min_tx: store
            .conn
            .query_row("SELECT MIN(id) FROM transactions", [], |r| r.get(0))?,
        canonicalization: "source-labels-sorted-ntriples/v1".into(),
        producer_version: env!("CARGO_PKG_VERSION").into(),
        producer_git_sha: env!("QUIPU_GIT_SHA").into(),
        producer_git_dirty: env!("QUIPU_GIT_DIRTY") == "true",
        graphs: receipts,
        complete: true,
    };
    let payload = Payload {
        source_receipt_sha256: crate::share::sha256(&receipt_bytes(&source_receipt)?),
        source_receipt,
        graphs,
    };
    let bytes = serde_json::to_vec(&payload).map_err(|e| invalid(e.to_string()))?;
    if bytes.len() > request.max_bytes.unwrap_or(MAX_BYTES).min(MAX_BYTES) {
        return Err(invalid(
            "acquisition exceeds response cap; completeness UNKNOWN",
        ));
    }
    check_deadline()?;
    read.commit()?;
    Ok(payload)
}

impl Payload {
    /// Verify integrity and exact expected source selection before import.
    /// The expected selection must come from trusted caller configuration.
    /// Hashes detect alteration against the receipt, not a hostile producer
    /// capable of replacing both payload and receipt; transport trust is separate.
    pub fn verify(&self, expected: &Request) -> Result<()> {
        let receipt = &self.source_receipt;
        let scope: BTreeSet<_> = expected.graphs.iter().collect();
        let actual: BTreeSet<_> = self.graphs.keys().collect();
        let stamped: BTreeSet<_> = receipt.graphs.keys().collect();
        if receipt.schema != "quipu-acquisition/v1"
            || !receipt.complete
            || receipt.history != "file-local-bitemporal-eavt"
            || receipt.canonicalization != "source-labels-sorted-ntriples/v1"
            || receipt.source_store_id != expected.source_store_id
            || receipt.requested_tx != expected.tx
            || receipt.applied_tx != expected.tx.unwrap_or(receipt.source_head_tx)
            || receipt.applied_tx < receipt.history_min_tx
            || receipt.applied_tx > receipt.source_head_tx
            || receipt.history_min_tx <= 0
            || scope.len() != expected.graphs.len()
            || scope != actual
            || scope != stamped
        {
            return Err(invalid(
                "acquisition receipt does not match expected source/pin/scope",
            ));
        }
        let hash = crate::share::sha256(&receipt_bytes(receipt)?);
        if hash != self.source_receipt_sha256 {
            return Err(invalid("acquisition receipt hash mismatch"));
        }
        for (iri, text) in &self.graphs {
            let graph = &receipt.graphs[iri];
            let triples = oxrdfio::RdfParser::from_format(oxrdfio::RdfFormat::NTriples)
                .for_reader(text.as_bytes())
                .map(|quad| quad.map(Triple::from).map_err(|e| invalid(e.to_string())))
                .collect::<Result<Vec<_>>>()?;
            let canonical = crate::rdf_export::serialize_triples_canonical(
                &triples,
                oxrdfio::RdfFormat::NTriples,
            )?;
            if canonical != text.as_bytes()
                || graph.sha256 != crate::share::sha256(text.as_bytes())
                || triples.len() != graph.triples
            {
                return Err(invalid("acquisition graph content/hash/count mismatch"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "acquisition_tests.rs"]
mod tests;
