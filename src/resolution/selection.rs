//! Resolve names only in score tiers that can affect the returned top-k.
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

use super::EntityCandidate;
use crate::{error::Result, store::Store};

#[derive(Clone)]
pub(super) enum Candidate {
    Known(EntityCandidate),
    Name {
        entity_id: i64,
        score: f64,
        exact: bool,
    },
}

impl Candidate {
    fn score(&self) -> f64 {
        match self {
            Self::Known(candidate) => candidate.score,
            Self::Name { score, .. } => *score,
        }
    }

    fn resolve(
        self,
        resolve: &mut impl FnMut(i64) -> Result<String>,
        iris: &mut HashMap<i64, String>,
    ) -> Result<EntityCandidate> {
        crate::time::check_request_deadline()?;
        match self {
            Self::Known(candidate) => Ok(candidate),
            Self::Name {
                entity_id,
                score,
                exact,
            } => {
                let iri = if let Some(iri) = iris.get(&entity_id) {
                    iri.clone()
                } else {
                    let iri = resolve(entity_id)?;
                    iris.insert(entity_id, iri.clone());
                    iri
                };
                Ok(EntityCandidate {
                    iri,
                    score,
                    matched_on: if exact {
                        "canonical_name:exact".into()
                    } else {
                        format!("canonical_name:jaro_winkler:{score:.2}")
                    },
                })
            }
        }
    }
}

/// The ordinary resolver orders ties by IRI; the in-lock delta recheck keeps
/// input order. Preserve both contracts rather than changing the latter here.
#[derive(Clone, Copy)]
pub(super) enum Ties {
    Iri,
    Input,
}

pub(super) fn select(
    store: &Store,
    candidates: Vec<Candidate>,
    excluded: &[String],
    top_k: usize,
    ties: Ties,
) -> Result<Vec<EntityCandidate>> {
    select_with(candidates, excluded, top_k, ties, |id| store.resolve(id))
}

fn select_with(
    mut candidates: Vec<Candidate>,
    excluded: &[String],
    top_k: usize,
    ties: Ties,
    mut resolve: impl FnMut(i64) -> Result<String>,
) -> Result<Vec<EntityCandidate>> {
    let excused: HashSet<&str> = excluded.iter().map(String::as_str).collect();
    let mut iris = HashMap::new();
    // Preserve the legacy total-resolution/error path for zero output and
    // unordered scores. Normal thresholded matches have finite scores.
    if top_k == 0 || candidates.iter().any(|c| c.score().is_nan()) {
        let mut resolved = candidates
            .into_iter()
            .map(|c| c.resolve(&mut resolve, &mut iris))
            .collect::<Result<Vec<_>>>()?;
        resolved.retain(|c| !excused.contains(c.iri.as_str()));
        resolved.sort_by(|a, b| {
            let order = b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal);
            match ties {
                Ties::Iri => order.then_with(|| a.iri.cmp(&b.iri)),
                Ties::Input => order,
            }
        });
        super::dedup_by_iri(&mut resolved);
        resolved.truncate(top_k);
        return Ok(resolved);
    }
    // Stable score sorting keeps vectors/previous hints before equally scored
    // label rows, exactly as before. Resolve the ENTIRE boundary tier: truncating
    // that tier first would silently change IRI tie order and exclusions.
    candidates.sort_by(|a, b| b.score().partial_cmp(&a.score()).unwrap_or(Ordering::Equal));
    let mut candidates = candidates.into_iter().peekable();
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    while let Some(first) = candidates.next() {
        crate::time::check_request_deadline()?;
        let score = first.score();
        let mut tier = vec![first.resolve(&mut resolve, &mut iris)?];
        while candidates.peek().is_some_and(|c| c.score() == score) {
            crate::time::check_request_deadline()?;
            if let Some(candidate) = candidates.next() {
                tier.push(candidate.resolve(&mut resolve, &mut iris)?);
            }
        }
        if matches!(ties, Ties::Iri) {
            tier.sort_by(|a, b| a.iri.cmp(&b.iri));
        }
        for candidate in tier {
            if !excused.contains(candidate.iri.as_str()) && seen.insert(candidate.iri.clone()) {
                output.push(candidate);
            }
        }
        if output.len() >= top_k {
            break;
        }
    }
    output.truncate(top_k);
    Ok(output)
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
