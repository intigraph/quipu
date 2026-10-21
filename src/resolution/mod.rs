//! Entity resolution — detect near-duplicate entities before writing.
//!
//! On entity writes (episode ingest or direct fact insert), the resolver:
//! 1. Computes an embedding of the entity's canonical name + properties.
//! 2. Queries the existing vector index for top-K nearest entities above a
//!    configurable similarity threshold.
//! 3. Runs canonical name matching (Jaro-Winkler) alongside the vector query
//!    to catch typos the embedding may miss.
//! 4. Drops any pairing the graph already excuses via `quipu:distinctFrom`.
//! 5. Returns merged, deduplicated candidates with scores and explanations.
//!
//! # What the two halves can see
//!
//! The name half reads [`Store::facts_source`] — the `UNION ALL` over the local
//! store and every attached layer — so on a composed store it sees the shared
//! reference layer a tenant is most likely to duplicate. It did not always: it
//! read the bare `facts` table, which is `main.facts`, so resolution against a
//! knowledge pack found nothing and the tenant minted a duplicate of an entity
//! the pack already defined. That is the fragmentation this module exists to
//! prevent, failing in the one deployment shape the composition design targets.
//!
//! The vector half cannot follow it there. `vectors` is a per-database table and
//! an attached pack may carry embeddings from a different model or dimension, so
//! unioning the indexes would turn a working search into a dimension-mismatch
//! error (`src/vector.rs` fails loud on that, deliberately). Rather than paper
//! over it, every result carries a [`VectorScope`] saying how much of the
//! composition the embedding half actually covered — because the failure that
//! cost us the first time was not the missing coverage, it was that missing
//! coverage and "no duplicates exist" returned the same empty list.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::namespace;
use crate::store::Store;
use crate::vector::VectorMatch;

pub(crate) mod cache;
#[cfg(test)]
mod cache_tests;
mod index;
mod jaro;
mod matching;
#[cfg(test)]
mod perf_tests;
mod selection;
pub(crate) use index::LabelIndex;
#[cfg(test)]
pub(crate) use index::LabelProfile;
#[cfg(test)]
pub(crate) use index::delta_sql;
#[cfg(test)]
mod tests;

pub use matching::{
    Contention, EpisodeResolution, NodeQuery, ResolutionGeneration, resolve_episode_nodes,
    resolve_nodes, revalidate_episode_resolution,
};

/// A candidate entity match from resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityCandidate {
    /// The IRI of the existing entity.
    pub iri: String,
    /// Similarity score (0.0 to 1.0).
    pub score: f64,
    /// How the match was found: `"embedding:0.91"` or `"canonical_name:jaro_winkler:0.95"`.
    pub matched_on: String,
}

/// How much of a composed store the embedding half of resolution searched.
///
/// See the module docs: this exists so that a caller can tell "the vector index
/// found nothing" from "the vector index was never asked about the layer your
/// duplicate is in".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VectorScope {
    /// No attachments — the vector index covers everything the name half reads.
    WholeStore,
    /// The store has attached layers whose vectors are NOT in the searched
    /// index. The name half covered them; the embedding half did not, so an
    /// embedding-only near-duplicate living in a layer will not be reported.
    LocalOnly {
        /// How many attached layers the embedding half could not see.
        attached_layers: usize,
    },
}

impl VectorScope {
    /// The scope a search against this store actually has.
    pub fn of(store: &Store) -> Self {
        match store.attachments.len() {
            0 => Self::WholeStore,
            n => Self::LocalOnly { attached_layers: n },
        }
    }

    /// Whether some part of the composition went unsearched by the vector half.
    pub fn is_partial(self) -> bool {
        matches!(self, Self::LocalOnly { .. })
    }
}

/// Result of entity resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionResult {
    /// Whether any candidates exceeded the threshold.
    pub has_matches: bool,
    /// Candidate entities sorted by descending score, then ascending IRI for ties.
    pub candidates: Vec<EntityCandidate>,
    /// How much of a composed store the embedding half covered.
    pub vector_scope: VectorScope,
}

/// Resolve a candidate entity name against the existing knowledge graph.
///
/// Combines vector similarity (embedding-based) and canonical name matching
/// (Jaro-Winkler) to find entities that may be duplicates of the proposed
/// name + properties. Results are merged, deduplicated by IRI, and sorted
/// by descending score.
///
/// This is the single-entity path. Resolving several entities at once — an
/// episode's nodes — goes through [`resolve_nodes`], which scans the label set
/// once for the whole batch instead of once per entity and can see contention
/// BETWEEN the entities being written.
///
/// # Errors
/// Propagates store, embedding and decode failures.
pub fn resolve_entity(
    store: &Store,
    name: &str,
    properties: &[(String, String)],
    threshold: f64,
    top_k: usize,
) -> Result<ResolutionResult> {
    let index = LabelIndex::build(store)?;
    resolve_one(store, &index, name, properties, threshold, top_k, &[])
}

/// One entity against an already-built label index.
///
/// `excluded` holds IRIs this entity is declared or recorded distinct from;
/// they are dropped from the candidate list before it is scored or refused.
fn resolve_one(
    store: &Store,
    index: &LabelIndex,
    name: &str,
    properties: &[(String, String)],
    threshold: f64,
    top_k: usize,
    excluded: &[String],
) -> Result<ResolutionResult> {
    // Phase 1: Vector similarity search.
    // Build text from name + properties, embed it, search the vector store.
    crate::time::check_request_deadline()?;
    let text = build_resolution_text(name, properties);

    let embedding = store.embed_query(&text)?;
    crate::time::check_request_deadline()?;
    let matches = match embedding {
        // Oversample to allow room after threshold filtering.
        Some(embedding) => Some(
            store
                .vector_store()
                .vector_search(&embedding, top_k * 3, None)?,
        ),
        None => None,
    };
    resolve_with_matches(
        store,
        index,
        name,
        threshold,
        top_k,
        excluded,
        matches.as_deref(),
    )
}

/// Phase 1 for a whole batch: embed every text, then ONE vector search call.
///
/// Result `i` is what [`resolve_one`] would have searched for `texts[i]`:
/// `None` where no embedding provider answers, else the same oversampled
/// top-`top_k * 3` matches (aegis-nmuwsi).
///
/// # Errors
/// Propagates embedding and store failures, and the request deadline.
pub(crate) fn batch_vector_matches(
    store: &Store,
    texts: &[String],
    top_k: usize,
) -> Result<Vec<Option<Vec<VectorMatch>>>> {
    let mut embeddings = Vec::with_capacity(texts.len());
    for text in texts {
        crate::time::check_request_deadline()?;
        embeddings.push(store.embed_query(text)?);
    }
    crate::time::check_request_deadline()?;
    let queries: Vec<&[f32]> = embeddings.iter().flatten().map(Vec::as_slice).collect();
    let mut searched = store
        .vector_store()
        .vector_search_batch(&queries, top_k * 3, None)?
        .into_iter();
    crate::time::check_request_deadline()?;
    Ok(embeddings
        .iter()
        .map(|e| e.as_ref().and_then(|_| searched.next()))
        .collect())
}

/// Everything after the vector search, given its matches (`None` = no provider).
pub(crate) fn resolve_with_matches(
    store: &Store,
    index: &LabelIndex,
    name: &str,
    threshold: f64,
    top_k: usize,
    excluded: &[String],
    vector_matches: Option<&[VectorMatch]>,
) -> Result<ResolutionResult> {
    let mut candidates = Vec::new();

    if let Some(matches) = vector_matches {
        for m in matches {
            if m.score >= threshold {
                let iri = store.resolve(store.canonical_id(m.entity_id)?)?;
                candidates.push(selection::Candidate::Known(EntityCandidate {
                    iri,
                    score: m.score,
                    matched_on: format!("embedding:{:.2}", m.score),
                }));
            }
        }
    }

    // Phase 2: Canonical name matching (Jaro-Winkler), over the whole
    // composition rather than the local store alone.
    candidates.extend(index.candidates(name, threshold)?);
    let candidates = selection::select(store, candidates, excluded, top_k, selection::Ties::Iri)?;

    Ok(ResolutionResult {
        has_matches: !candidates.is_empty(),
        candidates,
        vector_scope: VectorScope::of(store),
    })
}

/// Build embeddable text for resolution from a name and optional properties.
pub(crate) fn build_resolution_text(name: &str, properties: &[(String, String)]) -> String {
    let mut parts = vec![name.to_string()];
    for (k, v) in properties {
        parts.push(format!("{k}: {v}"));
    }
    parts.join(". ")
}

/// Whether `name` is a canonical `commit/<repo>/<hex-sha>` node name.
///
/// Git SHA-1 and SHA-256 object ids are 40 and 64 hex digits respectively;
/// seven digits is the conventional minimum useful abbreviated commit id.
pub(crate) fn is_slash_qualified_commit_id(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("commit/") else {
        return false;
    };
    let Some((repo, sha)) = rest.rsplit_once('/') else {
        return false;
    };
    !repo.is_empty()
        && !repo.split('/').any(str::is_empty)
        && (7..=64).contains(&sha.len())
        && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The IRIs `iri` is already recorded as `quipu:distinctFrom` in the graph.
///
/// This is what makes the override durable. A writer asserts the pairing once;
/// every later resolution of the same entity reads it back and stays silent
/// about that one pairing, without `strict_mode` having to be disabled for
/// every entity in the store.
///
/// # Errors
/// Propagates store and decode failures.
pub(crate) fn recorded_distinct_from(store: &Store, iri: &str) -> Result<Vec<String>> {
    // Both sides need every id the IRI denotes across the composition, for the
    // same reason the label scan does — see [`LabelIndex::build`].
    let subjects = store.lookup_all(iri)?;
    let preds = store.lookup_all(namespace::QUIPU_DISTINCT_FROM)?;
    if subjects.is_empty() || preds.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT v FROM {} WHERE e IN ({}) AND a IN ({}) AND valid_to IS NULL AND op = 1",
        store.facts_source(),
        id_list(&subjects),
        id_list(&preds),
    );
    let mut stmt = store.conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| row.get::<_, Vec<u8>>(0))?;

    let mut out = Vec::new();
    for row in rows {
        match crate::types::Value::from_bytes(&row?) {
            // The object is normally an IRI reference…
            Ok(crate::types::Value::Ref(id)) => out.push(store.resolve(id)?),
            // …but a writer that asserted it as a plain literal meant the same
            // thing, and silently ignoring that would put us back to an override
            // that looks asserted and does nothing.
            Ok(crate::types::Value::Str(s)) => out.push(s),
            _ => {}
        }
    }
    Ok(out)
}

/// Deduplicate candidates by IRI, keeping the highest-scoring entry for each.
pub(crate) fn dedup_by_iri(candidates: &mut Vec<EntityCandidate>) {
    // Candidates are already sorted by descending score, so the first
    // occurrence of each IRI is the highest-scoring one.
    let mut seen = HashSet::new();
    candidates.retain(|c| seen.insert(c.iri.clone()));
}

/// Term ids as a SQL `IN` list.
///
/// Inlined rather than bound because the arity varies with the number of
/// attached layers; the values are `i64`s the store itself produced, never
/// caller text, so there is nothing here to inject.
fn id_list(ids: &[i64]) -> String {
    ids.iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
