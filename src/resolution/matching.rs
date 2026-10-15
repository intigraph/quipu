//! Resolving a whole episode's nodes at once, and the contention between them.
//!
//! # Why this is not a stable matching
//!
//! Resolving N incoming nodes against M stored entities is a bipartite
//! assignment problem, and the reflex is to reach for stable matching. Two
//! reasons not to, recorded here because the question recurs:
//!
//! - **Stability is not the property wanted.** Gale-Shapley guarantees no
//!   blocking pair, not maximum total match quality, and it is proposer-optimal:
//!   running it with the incoming nodes proposing gives a different answer than
//!   with the stored entities proposing. There is no principled way to pick a
//!   proposing side here — the score is one symmetric number — so the asymmetry
//!   would be arbitrary. If this ever does assign, max-weight bipartite matching
//!   (Hungarian) is the right algorithm: symmetric, optimal, and trivially
//!   affordable at the node counts an episode carries.
//! - **Assigning is not this layer's job.** Quipu stores facts true at write
//!   time and leaves judgments to the reader. Picking which node "gets" a
//!   contested entity is a judgment, and one made from a similarity score that
//!   the caller can see and we cannot justify. So this pass DETECTS contention
//!   and reports it. It never resolves it.
//!
//! What was actually broken was narrower and worse than a missing algorithm:
//! nodes resolved one at a time in a loop, so nothing in the system could see
//! that two of them had claimed the same entity. Non-strict ingest emitted two
//! hints pointing at one IRI and looked like two independent near-misses;
//! strict ingest refused whichever node came first in the list and said nothing
//! about the other. The contention was invisible in exactly the case where it
//! matters most — the caller is about to fragment one entity into two.

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::store::Store;

use super::{EntityCandidate, LabelIndex, VectorScope, recorded_distinct_from};

/// One entity to resolve, as the caller already knows it.
#[derive(Debug)]
pub struct NodeQuery<'a> {
    /// The node's name, matched against stored labels.
    pub name: &'a str,
    /// The IRI this node will be written as. Used to read back any
    /// `quipu:distinctFrom` the graph already records for it.
    pub iri: String,
    /// Extra text folded into the embedding.
    pub properties: Vec<(String, String)>,
    /// IRIs this write DECLARES itself distinct from, before anything is stored.
    pub declared_distinct: &'a [String],
}

/// Two or more nodes in one write claiming the same existing entity.
///
/// "Claiming" means top candidate. A shared lower-ranked candidate is not a
/// conflict — two nodes can both resemble a third entity without either
/// asserting it is that entity — so only the match each node would act on
/// counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contention {
    /// The contested entity.
    pub iri: String,
    /// The node names claiming it, with the score each claimed it at, ordered
    /// by descending score.
    pub claimants: Vec<(String, f64)>,
}

/// Everything one resolution pass over an episode learned.
#[derive(Debug)]
pub struct EpisodeResolution {
    /// Per-node candidates (node name → candidates), for nodes with matches.
    pub hints: Vec<(String, Vec<EntityCandidate>)>,
    /// Entities claimed by more than one node in this write.
    pub contentions: Vec<Contention>,
    /// The strict-mode refusal message, if strict mode is on and a node matched.
    pub refusal: Option<String>,
    /// How much of a composed store the embedding half covered.
    pub vector_scope: VectorScope,
}

/// Resolve every node of a write in one pass.
///
/// Scans the label set once for the whole batch (see [`LabelIndex`]) instead of
/// once per node, and compares the results across nodes so contention between
/// them is visible.
///
/// # Errors
/// Propagates store, embedding and decode failures.
pub fn resolve_nodes(
    store: &Store,
    nodes: &[NodeQuery<'_>],
    threshold: f64,
    top_k: usize,
    strict_mode: bool,
) -> Result<EpisodeResolution> {
    crate::time::check_request_deadline()?;
    let index = LabelIndex::build(store)?;
    let mut hints = Vec::new();
    let mut refusal = None;
    // (contested iri, claiming node, score) — accumulated across nodes.
    let mut claims: Vec<(String, String, f64)> = Vec::new();
    // Phase 1 for every node in one vector scan instead of one scan per node.
    let texts: Vec<String> = nodes
        .iter()
        .map(|n| super::build_resolution_text(n.name, &n.properties))
        .collect();
    let vector_matches = super::batch_vector_matches(store, &texts, top_k)?;

    for (node, matches) in nodes.iter().zip(vector_matches.iter()) {
        crate::time::check_request_deadline()?;
        // A write may declare a pairing distinct inline; the graph may already
        // record it from an earlier write. Both excuse the same pairing, and
        // the durable one is what stops a strict refusal from recurring.
        let mut excused = recorded_distinct_from(store, &node.iri)?;
        excused.extend(node.declared_distinct.iter().cloned());

        let result = super::resolve_with_matches(
            store,
            &index,
            node.name,
            threshold,
            top_k,
            &excused,
            matches.as_deref(),
        )?;

        if !result.has_matches {
            continue;
        }
        let top = &result.candidates[0];
        claims.push((top.iri.clone(), node.name.to_string(), top.score));
        if strict_mode && refusal.is_none() {
            refusal = Some(format!(
                "entity resolution: '{}' matches existing entity '{}' \
                 (score: {:.2}, matched by: {}). Use an existing IRI, or assert \
                 quipu:distinctFrom <{}> on this entity to override.",
                node.name, top.iri, top.score, top.matched_on, top.iri,
            ));
        }
        hints.push((node.name.to_string(), result.candidates));
    }

    Ok(EpisodeResolution {
        hints,
        contentions: contentions(claims),
        refusal,
        vector_scope: VectorScope::of(store),
    })
}

/// Group claims by contested entity, keeping only the genuinely contested ones.
fn contentions(mut claims: Vec<(String, String, f64)>) -> Vec<Contention> {
    // Sort by IRI, then descending score, so each group is contiguous and its
    // claimants are already in the order the report wants them.
    claims.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut out: Vec<Contention> = Vec::new();
    for (iri, name, score) in claims {
        match out.last_mut() {
            Some(last) if last.iri == iri => last.claimants.push((name, score)),
            _ => out.push(Contention {
                iri,
                claimants: vec![(name, score)],
            }),
        }
    }
    out.retain(|c| c.claimants.len() > 1);
    out
}

/// Resolve an episode's nodes under the ingest options that govern the write.
///
/// Lives here rather than in `episode` so the per-node plumbing — deriving each
/// node's IRI, folding its description into the embedding text, threading its
/// declared non-identities through — sits next to the pass that consumes it.
///
/// # Errors
/// Propagates store, embedding and decode failures.
pub fn resolve_episode_nodes(
    store: &Store,
    nodes: &[crate::episode::Node],
    base_ns: &str,
    opts: &crate::episode::IngestResolutionOpts,
) -> Result<EpisodeResolution> {
    let queries: Vec<NodeQuery<'_>> = nodes
        .iter()
        .map(|n| NodeQuery {
            name: &n.name,
            iri: crate::episode::node_iri(&n.name, base_ns),
            properties: n
                .description
                .as_ref()
                .map(|d| vec![("description".to_string(), d.clone())])
                .unwrap_or_default(),
            declared_distinct: &n.distinct_from,
        })
        .collect();
    resolve_nodes(
        store,
        &queries,
        opts.threshold,
        opts.top_k,
        opts.strict_mode,
    )
}

/// What a resolution pass read, so the writer can tell whether it is stale.
///
/// A two-phase `/episode` (aegis-xi0) resolves on a read connection and commits
/// under the writer. Between the two, another write may commit. The generation
/// is the last transaction id plus the set of mounted frozen layers: equal means
/// nothing the matcher reads can have changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionGeneration {
    /// `MAX(id)` of `transactions` when the pass started.
    pub tx: i64,
    /// Mounted frozen-pack aliases, in order.
    pub frozen: Vec<String>,
}

impl ResolutionGeneration {
    /// The generation `store` currently reads.
    ///
    /// # Errors
    /// Propagates store failures.
    pub fn of(store: &Store) -> Result<Self> {
        Ok(Self {
            tx: store.latest_tx_id()?,
            frozen: store
                .attachments
                .iter()
                .filter(|a| a.alias.starts_with("fz_"))
                .map(|a| a.alias.clone())
                .collect(),
        })
    }
}

/// Bring a resolution computed at `read_at` up to date under the writer.
///
/// Returns `None` when the delta cannot be bounded (the frozen layers changed),
/// and the caller must resolve again in full. Otherwise every node is re-checked
/// against the labels asserted after `read_at.tx` with the same matcher and
/// threshold, and the graph's recorded `distinctFrom` is re-read, so a
/// near-duplicate or an excusal committed between the phases is reflected.
/// Embedding matches against entities committed in between are not added: their
/// vectors are written after their own commit, so the in-lock path cannot rely
/// on them either.
///
/// # Errors
/// Propagates store and decode failures.
pub fn revalidate_episode_resolution(
    store: &Store,
    nodes: &[crate::episode::Node],
    base_ns: &str,
    opts: &crate::episode::IngestResolutionOpts,
    read_at: &ResolutionGeneration,
    resolved: EpisodeResolution,
) -> Result<Option<EpisodeResolution>> {
    let now = ResolutionGeneration::of(store)?;
    if now == *read_at {
        return Ok(Some(resolved));
    }
    if now.frozen != read_at.frozen || opts.strict_mode {
        return Ok(None);
    }
    crate::time::check_request_deadline()?;
    let delta = super::LabelIndex::build_since(store, read_at.tx)?;
    let mut previous: std::collections::HashMap<String, Vec<super::EntityCandidate>> =
        resolved.hints.into_iter().collect();
    let mut hints = Vec::new();
    let mut claims: Vec<(String, String, f64)> = Vec::new();
    for node in nodes {
        crate::time::check_request_deadline()?;
        let iri = crate::episode::node_iri(&node.name, base_ns);
        let mut excused = recorded_distinct_from(store, &iri)?;
        excused.extend(node.distinct_from.iter().cloned());
        let mut candidates = previous.remove(&node.name).unwrap_or_default();
        candidates.extend(delta.candidates(store, &node.name, opts.threshold)?);
        if !excused.is_empty() {
            let excused: std::collections::HashSet<&str> =
                excused.iter().map(String::as_str).collect();
            candidates.retain(|c| !excused.contains(c.iri.as_str()));
        }
        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        super::dedup_by_iri(&mut candidates);
        candidates.truncate(opts.top_k);
        if let Some(top) = candidates.first() {
            claims.push((top.iri.clone(), node.name.clone(), top.score));
            hints.push((node.name.clone(), candidates));
        }
    }
    Ok(Some(EpisodeResolution {
        hints,
        contentions: contentions(claims),
        refusal: None,
        vector_scope: resolved.vector_scope,
    }))
}
