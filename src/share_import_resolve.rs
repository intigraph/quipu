//! Entity resolution for `/import`, computed once per bundle and off the writer.
//!
//! # Why this is its own pass (aegis-oa5)
//!
//! `/import` resolved every labelled subject of a share with
//! [`crate::resolve_entity`], one call per label, inside the single writer.
//! Each call rebuilt the label index: the shared label cache is bypassed inside a
//! transaction, so every call rescanned every label in the store and paid one
//! embedding and one vector search. MEASURED on a copy of the production store: one
//! call took 1.19-1.33 s. A 25,446-triple share has 1,985 labels, so a single
//! import held the writer for over 30 minutes. Production writes queued behind it
//! twice on 2026-10-22, and a client timeout did not release the writer.
//!
//! The fix has two halves:
//!
//! - **One pass per bundle.** The label set is scanned once, and every label's
//!   vector search runs in one batch, the same way [`crate::resolution::resolve_nodes`]
//!   serves an episode. The per-label answer is the one `resolve_entity` would
//!   have given. Candidates are kept untruncated until the end, so that a
//!   revalidation can drop a stale one without hiding the next.
//! - **Off the writer.** [`prepare`] runs on a read connection. Under the writer,
//!   [`revalidate`] re-checks only the labels asserted or retracted after the
//!   read generation, as a two-phase `/episode` does (aegis-xi0). The rewrite that
//!   `accept_exact` performs is destructive, so a candidate whose label changed in
//!   between is dropped rather than trusted.

use std::collections::{BTreeMap, HashSet};

use oxrdf::Triple;

use crate::error::Result;
use crate::namespace;
use crate::resolution::{
    EntityCandidate, LabelIndex, ResolutionGeneration, batch_vector_matches, dedup_by_iri,
    resolve_with_matches,
};
use crate::share_import::{ImportMatch, ImportResolution, ShareImportRequest};
use crate::store::Store;

/// The fuzzy-match threshold `/import` has always used.
const THRESHOLD: f64 = 0.85;
/// Candidates reported per foreign label.
const TOP_K: usize = 5;

/// One foreign subject, its label, and every candidate above the threshold.
#[derive(Debug, Clone)]
pub(crate) struct LabelCandidates {
    foreign: String,
    label: String,
    candidates: Vec<EntityCandidate>,
    /// Name candidates found on the read and dropped under the writer because
    /// their label changed in between. Reported, never silently lost.
    dropped: Vec<EntityCandidate>,
}

/// A share parsed and resolved on a read connection, waiting for the writer.
#[derive(Debug)]
pub struct PreparedImport {
    share_id: String,
    graph_hash: String,
    pub(crate) triples: Vec<Triple>,
    labels: Vec<LabelCandidates>,
    read_at: ResolutionGeneration,
}

impl PreparedImport {
    /// Whether this resolution was computed for `request`.
    pub(crate) fn is_for(&self, request: &ShareImportRequest) -> bool {
        self.share_id == request.manifest.share_id && self.graph_hash == request.manifest.graph_hash
    }
}

/// Phase 0: parse the share and resolve its labels without the writer.
///
/// The share is not verified here. Verification and every other admission check
/// run under the writer, so a refusal there leaves nothing behind, as before.
///
/// # Errors
/// Parse, store and embedding failures, and the request deadline.
pub fn prepare(store: &Store, request: &ShareImportRequest) -> Result<PreparedImport> {
    crate::time::check_request_deadline()?;
    let triples = crate::share_import::parse_triples(&request.export_ntriples)?;
    let read_at = ResolutionGeneration::of(store)?;
    let labels = resolve(store, &crate::share_import::labels(&triples))?;
    Ok(PreparedImport {
        share_id: request.manifest.share_id.clone(),
        graph_hash: request.manifest.graph_hash.clone(),
        triples,
        labels,
        read_at,
    })
}

/// Resolve every label of a bundle in one pass over the label set.
///
/// # Errors
/// Store and embedding failures, and the request deadline.
pub(crate) fn resolve(
    store: &Store,
    labels: &BTreeMap<String, String>,
) -> Result<Vec<LabelCandidates>> {
    crate::time::check_request_deadline()?;
    let index = LabelIndex::build(store)?;
    let texts: Vec<String> = labels.values().cloned().collect();
    let matches = batch_vector_matches(store, &texts, TOP_K)?;
    let mut out = Vec::with_capacity(labels.len());
    for ((foreign, label), matches) in labels.iter().zip(matches.iter()) {
        crate::time::check_request_deadline()?;
        // Untruncated: `finish` applies TOP_K after any revalidation.
        let result = resolve_with_matches(
            store,
            &index,
            label,
            THRESHOLD,
            usize::MAX,
            &[],
            matches.as_deref(),
        )?;
        out.push(LabelCandidates {
            foreign: foreign.clone(),
            label: label.clone(),
            candidates: result.candidates,
            dropped: Vec::new(),
        });
    }
    Ok(out)
}

/// Phase 1, under the writer: bring a prepared resolution up to date.
///
/// Returns `None` when the mounted frozen layers changed, so the delta cannot be
/// bounded; the caller then resolves in full. Otherwise a name candidate whose
/// label was asserted or retracted after the read is dropped, and the labels
/// asserted since are matched again. Embedding candidates are kept: a vector is
/// written after its own commit, so the in-lock path could not see a newer one
/// either.
///
/// # Errors
/// Store and decode failures, and the request deadline.
pub(crate) fn revalidate(
    store: &Store,
    prepared: &PreparedImport,
) -> Result<Option<Vec<LabelCandidates>>> {
    let now = ResolutionGeneration::of(store)?;
    if now == prepared.read_at {
        return Ok(Some(prepared.labels.clone()));
    }
    if now.frozen != prepared.read_at.frozen {
        return Ok(None);
    }
    crate::time::check_request_deadline()?;
    let delta = LabelIndex::build_since(store, prepared.read_at.tx)?;
    let touched = touched_since(store, &delta, prepared.read_at.tx)?;
    let mut out = Vec::with_capacity(prepared.labels.len());
    for entry in &prepared.labels {
        crate::time::check_request_deadline()?;
        let (dropped, mut candidates): (Vec<EntityCandidate>, Vec<EntityCandidate>) =
            entry.candidates.iter().cloned().partition(|c| {
                c.matched_on.starts_with("canonical_name") && touched.contains(&c.iri)
            });
        candidates.extend(delta.candidates(store, &entry.label, THRESHOLD)?);
        sort(&mut candidates);
        out.push(LabelCandidates {
            candidates,
            dropped,
            ..entry.clone()
        });
    }
    Ok(Some(out))
}

/// Entities whose label row was retracted after `?1`; `?2` is the label id.
const RETRACTED_LABELS_SQL: &str =
    "SELECT DISTINCT e FROM main.facts WHERE retracted_tx > ?1 AND +a = ?2";

/// IRIs whose label was asserted or retracted in a transaction after `tx`.
fn touched_since(store: &Store, delta: &LabelIndex, tx: i64) -> Result<HashSet<String>> {
    let mut touched = HashSet::new();
    for id in delta.entity_ids() {
        touched.insert(store.resolve(id)?);
    }
    // Only the local store can change between the phases: a change to the
    // frozen layers is handled above. A retraction closes the old row and stamps
    // `retracted_tx`, which `idx_retracted_tx` serves. The `+a` keeps the planner
    // off the label index, which has no statistics to stop it: MEASURED on a
    // production-size copy, 49.8 s by `idx_aevt` against 0.006 s by this one.
    let Some(label) = store.lookup(&format!("{}label", namespace::RDFS))? else {
        return Ok(touched);
    };
    let mut stmt = store.conn.prepare(RETRACTED_LABELS_SQL)?;
    let rows = stmt.query_map(rusqlite::params![tx, label], |row| row.get::<_, i64>(0))?;
    for row in rows {
        touched.insert(store.resolve(store.canonical_id(row?)?)?);
    }
    Ok(touched)
}

/// The tie rule `resolve_with_matches` applies, then dedup.
fn sort(candidates: &mut Vec<EntityCandidate>) {
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.iri.cmp(&b.iri))
    });
    dedup_by_iri(candidates);
}

/// The report and the exact-match rewrites, from final candidates.
///
/// This is the decision `resolve_and_rewrite` made per label, unchanged: only a
/// top candidate that is an exact canonical-name match is a merge, and it is
/// applied only with `accept_exact` (aegis-i48b9w).
pub(crate) fn finish(
    labels: Vec<LabelCandidates>,
    accept_exact: bool,
) -> (ImportResolution, BTreeMap<String, String>) {
    let mut report = ImportResolution::default();
    let mut replacements = BTreeMap::new();
    for LabelCandidates {
        foreign,
        mut candidates,
        dropped,
        ..
    } in labels
    {
        for candidate in dropped {
            report.revalidation_dropped.push(ImportMatch {
                foreign: foreign.clone(),
                local: candidate.iri,
                score: Some(candidate.score),
                matched_on: Some(candidate.matched_on),
            });
        }
        candidates.truncate(TOP_K);
        let Some(top) = candidates.first() else {
            report.unmatched.push(foreign);
            continue;
        };
        if top.score == 1.0 && top.matched_on == "canonical_name:exact" {
            if accept_exact {
                replacements.insert(foreign.clone(), top.iri.clone());
            }
            report.exact_merges.push(ImportMatch {
                foreign,
                local: top.iri.clone(),
                score: Some(top.score),
                matched_on: Some(top.matched_on.clone()),
            });
        } else {
            for candidate in candidates {
                report.candidates.push(ImportMatch {
                    foreign: foreign.clone(),
                    local: candidate.iri,
                    score: Some(candidate.score),
                    matched_on: Some(candidate.matched_on),
                });
            }
        }
    }
    (report, replacements)
}

#[cfg(test)]
#[path = "share_import_resolve_tests.rs"]
mod tests;
