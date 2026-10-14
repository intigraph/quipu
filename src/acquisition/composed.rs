//! Bounded qualified-main acquisition on an actually composed connection.
//! Attachments remain mounted, fenced, and explicitly outside the data scope.

use super::{GraphReceipt, MAX_BYTES, Receipt as MainReceipt, invalid, main_graph, provider_fence};
use crate::{Error, Result, Store};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Separate from acquisition/v1, which continues to refuse attachments.
pub const CONTRACT: &str = "quipu-acquisition/composed-v1";
/// Total provider bytes hashed per call; overflow refuses before a complete receipt.
pub const PROVIDER_BYTES: u64 = 256 * 1024 * 1024;
/// Fixed census cap, independent of response bytes.
pub const MAX_PROVIDERS: usize = 16;
pub(super) const MAX_PROVIDER_GRAPHS: usize = 512;

/// Exact graph selection and optional expected captured composition.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Trusted consumer's expected main lineage.
    pub source_store_id: String,
    /// Exact main-owned committed graph IRIs; required unknown graphs refuse.
    pub graphs: Vec<String>,
    /// Main pin only; never applied to another provider's clock.
    #[serde(default, deserialize_with = "super::strict_tx")]
    pub tx: Option<i64>,
    /// Observed topology generation; required with any explicit main pin.
    pub expected_generation: Option<String>,
    /// May only lower the response ceiling.
    pub max_bytes: Option<usize>,
}

/// One excluded but retained immutable provider, with opaque physical identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    /// Runtime alias, not lineage or a pathname.
    pub alias: String,
    /// Provider's own persistent lineage.
    pub store_id: String,
    /// Opaque handle generation; incorporates inode/change identity, not its path.
    pub instance: String,
    /// SHA-256 of all retained database bytes under the kernel lease.
    pub file_sha256: String,
    /// Manifest digest bound to this file generation.
    pub manifest_sha256: String,
    /// Provider's local head, never a main applied pin.
    pub head_tx: i64,
    /// This provider's owned term space.
    pub term_space: i64,
    /// Complete bounded named graph inventory.
    pub graphs: Vec<String>,
    /// Explicit data-scope exclusion.
    pub exclusion: String,
}

/// Actual runtime census and qualified reader semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    /// Captured topology/dependency digest, distinct from a main transaction.
    pub generation: String,
    /// Canonical connection incarnation, not a path or copied lineage.
    pub incarnation: String,
    /// Earliest main pin observed with this scope's fenced topology.
    pub history_min_tx: i64,
    /// The actual coherent cut mechanism, not an arbitrary provider vector.
    pub coherence: String,
    /// Direct-main identity semantics, explicitly distinct from composed /query.
    pub reader: String,
    /// Every active attachment, including excluded providers.
    pub providers: Vec<Provider>,
    /// Selected graph ownership and policy/identity dependency digest.
    pub ownership_sha256: String,
}

/// Hash-bound source receipt; artifact-local identifiers remain separate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    /// Recognized composed contract.
    pub schema: String,
    /// Main lineage and applied pin, not a composed global clock.
    pub main: MainReceipt,
    /// Bound runtime census and history floor.
    pub composition: Composition,
    /// Set only after final fences, time, rows and byte checks.
    pub complete: bool,
}

/// Complete canonical graph payload with its independent source receipt.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    /// Source receipt, retained unchanged by consumers.
    pub source_receipt: Receipt,
    /// Hash of recursively key-sorted UTF-8 JSON of the receipt.
    pub source_receipt_sha256: String,
    /// Canonical, acquisition-wide blank-node scope.
    pub graphs: BTreeMap<String, String>,
}

pub(crate) fn new_incarnation() -> String {
    use ring::rand::SecureRandom;
    let mut bytes = [0_u8; 32];
    if ring::rand::SystemRandom::new().fill(&mut bytes).is_ok() {
        crate::share::sha256(&bytes)
    } else {
        String::new() // acquisition refuses if entropy is unavailable
    }
}

impl Store {
    /// Invalidate even a failed transition: equality after remount is not ABA
    /// proof. Called before the canonical handle changes its mounted topology.
    pub(crate) fn invalidate_acquisition_topology(&mut self) {
        self.acquisition_anchors.borrow_mut().clear();
        self.acquisition_incarnation = new_incarnation();
    }
}

pub(super) fn digest<T: Serialize>(value: &T) -> Result<String> {
    let bytes =
        serde_json::to_vec(&serde_json::to_value(value).map_err(|e| invalid(e.to_string()))?)
            .map_err(|e| invalid(e.to_string()))?;
    Ok(crate::share::sha256(&bytes))
}

/// Acquire an exact MAIN scope with all active immutable providers fenced.
/// History begins at this incarnation's first observation of that topology.
pub fn acquire(store: &Store, request: &Request) -> Result<Payload> {
    acquire_with_control(store, request, || Ok(()))
}

fn acquire_with_control(
    store: &Store,
    request: &Request,
    after_cut: impl FnOnce() -> Result<()>,
) -> Result<Payload> {
    let started = crate::time::Stopwatch::start();

    let check = || {
        if started.elapsed_ms() >= 10000
            || crate::time::request_deadline().is_some_and(|d| d.passed())
        {
            return Err(Error::QueryTimeout {
                elapsed_ms: started.elapsed_ms(),
                limit_ms: 10000,
            });
        }
        Ok(())
    };
    check()?;
    let scope: BTreeSet<_> = request.graphs.iter().collect();
    if scope.is_empty()
        || scope.len() > 16
        || scope.len() != request.graphs.len()
        || store.attachments.len() > MAX_PROVIDERS
    {
        return Err(invalid(
            "acquisition requires complete bounded unique scope/census",
        ));
    }
    let paths: Vec<_> = store.attachments.iter().map(|a| a.path.clone()).collect();
    let fence = provider_fence::Fence::new(&paths)?;
    let read = store.conn.unchecked_transaction()?;
    let source_store_id = store.store_id()?;
    if store.acquisition_incarnation.is_empty() || source_store_id != request.source_store_id {
        return Err(invalid("acquisition source mismatch"));
    }
    let head = store.latest_tx_id()?;
    let requested_tx = request.tx;
    if let Some(tx) = requested_tx
        && (tx <= 0
            || tx > head
            || !store
                .conn
                .prepare("SELECT 1 FROM main.transactions WHERE id=?1")?
                .exists([tx])?)
    {
        return Err(invalid("main pin is not an existing committed transaction"));
    }
    if requested_tx.is_some() && request.expected_generation.is_none() {
        return Err(invalid(
            "explicit main pin requires its observed composition generation",
        ));
    }
    let applied_tx = requested_tx.unwrap_or(head);
    if applied_tx <= 0 {
        return Err(invalid("source has no committed main transaction"));
    }
    let providers = super::census::providers(store, &fence, &check)?;
    let mut selected = Vec::new();
    let mut dependencies = BTreeMap::new();
    for iri in &request.graphs {
        check()?;
        oxrdf::NamedNode::new(iri).map_err(|e| invalid(e.to_string()))?;
        if providers
            .iter()
            .any(|p| p.graphs.contains(iri) || &p.alias == iri)
        {
            return Err(invalid("selected graph overlaps an attachment"));
        }
        let (id, metadata) = main_graph::metadata(store, iri)?;
        selected.push((iri, id));
        dependencies.insert(iri, metadata);
    }
    let ownership_sha256 = digest(&(
        dependencies,
        (
            &store.labels_config.min_freshness,
            &store.labels_config.min_trust_rank,
            &store.labels_config.min_trust_chain,
            &store.labels_config.deny_policy_tokens,
            &store.labels_config.deny_data_kinds,
        ),
    ))?;
    let fingerprint = digest(&(
        source_store_id.as_str(),
        &store.acquisition_incarnation,
        &providers,
        &ownership_sha256,
    ))?;
    if request
        .expected_generation
        .as_ref()
        .is_some_and(|expected| expected != &fingerprint)
    {
        return Err(invalid("composition generation mismatch"));
    }
    let key: Vec<_> = scope.into_iter().cloned().collect();
    let floor = {
        let mut anchors = store.acquisition_anchors.borrow_mut();
        if anchors.len() >= 16 && !anchors.contains_key(&key) {
            return Err(invalid("acquisition history scope budget exceeded"));
        }
        let anchor = anchors
            .entry(key)
            .or_insert_with(|| (fingerprint.clone(), head));
        if anchor.0 != fingerprint {
            *anchor = (fingerprint.clone(), head);
        }
        anchor.1
    };
    if applied_tx < floor {
        return Err(invalid(
            "historical composition not observed at requested main pin",
        ));
    }
    after_cut()?;
    let mut graphs = BTreeMap::new();
    let mut receipts = BTreeMap::new();
    let mut bytes_left = request.max_bytes.unwrap_or(MAX_BYTES).min(MAX_BYTES);
    for (iri, g) in selected {
        let text = main_graph::read(store, g, applied_tx, bytes_left, &check)?;
        bytes_left -= text.len();
        receipts.insert(
            iri.clone(),
            GraphReceipt {
                triples: text.lines().count(),
                sha256: crate::share::sha256(text.as_bytes()),
            },
        );
        graphs.insert(iri.clone(), text);
    }
    let main = MainReceipt {
        schema: "quipu-acquisition/main-direct-v1".into(),
        source_store_id,
        source_head_tx: head,
        requested_tx,
        applied_tx,
        history: "file-local-bitemporal-eavt".into(),
        history_min_tx: store
            .conn
            .query_row("SELECT MIN(id) FROM main.transactions", [], |r| r.get(0))?,
        canonicalization: "source-labels-sorted-ntriples/v1".into(),
        producer_version: env!("CARGO_PKG_VERSION").into(),
        producer_git_sha: env!("QUIPU_GIT_SHA").into(),
        producer_git_dirty: env!("QUIPU_GIT_DIRTY") == "true",
        graphs: receipts,
        complete: true,
    };
    let receipt = Receipt {
        schema: CONTRACT.into(),
        main,
        composition: Composition {
            generation: fingerprint,
            incarnation: store.acquisition_incarnation.clone(),
            history_min_tx: floor,
            coherence: "main-read-transaction+retained-provider-leases/v1".into(),
            reader: "qualified-main-committed/v1".into(),
            providers,
            ownership_sha256,
        },
        complete: true,
    };
    let payload = Payload {
        source_receipt_sha256: digest(&receipt)?,
        source_receipt: receipt,
        graphs,
    };
    if serde_json::to_vec(&payload)
        .map_err(|e| invalid(e.to_string()))?
        .len()
        > request.max_bytes.unwrap_or(MAX_BYTES).min(MAX_BYTES)
    {
        return Err(invalid(
            "acquisition response cap exceeded; completeness UNKNOWN",
        ));
    }
    check()?;
    fence.check()?;
    for a in &store.attachments {
        provider_fence::mounted_file_unchanged(store, &a.alias)?;
    }
    read.commit()?;
    Ok(payload)
}

impl Payload {
    /// Verify original selection, composition expectation, all bytes and pins.
    pub fn verify(&self, expected: &Request) -> Result<()> {
        let r = &self.source_receipt;
        let c = &r.composition;
        let m = &r.main;
        if r.schema != CONTRACT
            || !r.complete
            || m.schema != "quipu-acquisition/main-direct-v1"
            || c.reader != "qualified-main-committed/v1"
            || c.coherence != "main-read-transaction+retained-provider-leases/v1"
            || c.history_min_tx <= 0
            || m.applied_tx < c.history_min_tx
            || c.history_min_tx > m.source_head_tx
            || c.incarnation.is_empty()
            || digest(&(
                m.source_store_id.as_str(),
                &c.incarnation,
                &c.providers,
                &c.ownership_sha256,
            ))? != c.generation
            || c.providers.len() > MAX_PROVIDERS
            || expected
                .expected_generation
                .as_ref()
                .is_some_and(|g| g != &c.generation)
            || digest(r)? != self.source_receipt_sha256
        {
            return Err(invalid("composition receipt mismatch"));
        }
        let mut aliases = BTreeSet::new();
        let mut instances = BTreeSet::new();
        let mut spaces = BTreeSet::new();
        for p in &c.providers {
            if !aliases.insert(&p.alias)
                || !instances.insert(&p.instance)
                || !spaces.insert(p.term_space)
                || p.store_id.is_empty()
                || p.instance.is_empty()
                || p.file_sha256.is_empty()
                || p.manifest_sha256.is_empty()
                || p.exclusion != "outside-qualified-main-scope"
                || p.graphs.len() > MAX_PROVIDER_GRAPHS
                || p.graphs.iter().any(|g| expected.graphs.contains(g))
            {
                return Err(invalid("composition census mismatch"));
            }
        }
        let scope: BTreeSet<_> = expected.graphs.iter().collect();
        let actual: BTreeSet<_> = self.graphs.keys().collect();
        let stamped: BTreeSet<_> = m.graphs.keys().collect();
        if !m.complete
            || m.history != "file-local-bitemporal-eavt"
            || m.canonicalization != "source-labels-sorted-ntriples/v1"
            || m.source_store_id != expected.source_store_id
            || m.requested_tx != expected.tx
            || (expected.tx.is_some() && expected.expected_generation.is_none())
            || m.applied_tx != expected.tx.unwrap_or(m.source_head_tx)
            || m.history_min_tx <= 0
            || m.applied_tx < m.history_min_tx
            || m.applied_tx > m.source_head_tx
            || scope.is_empty()
            || scope.len() > 16
            || scope.len() != expected.graphs.len()
            || scope != actual
            || scope != stamped
        {
            return Err(invalid("composed main pin/source/scope mismatch"));
        }
        for (iri, text) in &self.graphs {
            let triples = oxrdfio::RdfParser::from_format(oxrdfio::RdfFormat::NTriples)
                .for_reader(text.as_bytes())
                .map(|q| {
                    q.map(oxrdf::Triple::from)
                        .map_err(|e| invalid(e.to_string()))
                })
                .collect::<Result<Vec<_>>>()?;
            let canonical = crate::rdf_export::serialize_triples_canonical(
                &triples,
                oxrdfio::RdfFormat::NTriples,
            )?;
            if canonical != text.as_bytes()
                || triples.len() != m.graphs[iri].triples
                || crate::share::sha256(text.as_bytes()) != m.graphs[iri].sha256
            {
                return Err(invalid("composed graph content/hash/count mismatch"));
            }
        }
        if serde_json::to_vec(self)
            .map_err(|e| invalid(e.to_string()))?
            .len()
            > expected.max_bytes.unwrap_or(MAX_BYTES).min(MAX_BYTES)
        {
            return Err(invalid("composed response exceeds expected byte cap"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "composed_tests.rs"]
mod tests;
