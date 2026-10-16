//! Context admission is separate from ranking. Candidates remain diagnostic;
//! agent-facing consumers use only `context_results` when admission succeeds.
use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::{Error, Result};

const DEFAULT_FLOOR: f64 = 0.52;
const MIN_COVERAGE: f64 = 0.5;
const STOP: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "did", "do", "does", "for", "from", "how",
    "i", "in", "is", "it", "me", "of", "on", "or", "please", "show", "tell", "that", "the", "this",
    "to", "was", "were", "what", "when", "where", "which", "who", "why", "with",
];

pub(super) struct Policy {
    floor: f64,
}

impl Policy {
    pub(super) fn parse(input: &Value) -> Result<Self> {
        let floor = input
            .get("context_similarity_floor")
            .map_or(Some(DEFAULT_FLOOR), Value::as_f64)
            .filter(|f| f.is_finite() && (0.0..=1.0).contains(f))
            .ok_or_else(|| {
                Error::InvalidValue("context_similarity_floor must be finite and in [0,1]".into())
            })?;
        Ok(Self { floor })
    }

    pub(super) fn annotate(&self, input: &Value, response: &mut Value) -> Result<()> {
        let query = input["query"].as_str().unwrap_or("");
        let rows = response["results"].as_array().cloned().ok_or_else(|| {
            Error::InvalidValue("search response results must be an array".into())
        })?;
        let terms = if query.len() <= 4096 {
            query_terms(query)
        } else {
            Vec::new()
        };
        let identifiers = if query.len() <= 4096 {
            identifiers(query)
        } else {
            Vec::new()
        };
        let evaluations: Vec<_> = rows
            .iter()
            .map(|row| self.evaluate(row, &terms, &identifiers))
            .collect();
        // A lower-ranked coincidence cannot overrule an uncorroborated top hit.
        let reason = if query.len() > 4096 {
            "query_too_long"
        } else if terms.is_empty() {
            "query_terms_unavailable"
        } else if rows.is_empty() {
            "no_candidates"
        } else {
            match evaluations[0].reason {
                "below_similarity_floor"
                    if evaluations
                        .iter()
                        .all(|e| e.reason == "below_similarity_floor") =>
                {
                    "no_result_above_floor"
                }
                "below_similarity_floor" => "top_result_below_floor",
                reason => reason,
            }
        };
        let available = reason == "sufficient_context";
        let accepted: Vec<_> = rows
            .iter()
            .zip(&evaluations)
            .filter(|(_, eval)| available && eval.reason == "sufficient_context")
            .map(|(row, _)| row.clone())
            .collect();
        response["insufficient_context"] = json!(!available);
        response["context_results"] = json!(accepted);
        response["context"] = json!({
            "contract":"quipu-context/v1",
            "outcome":if available {"CONTEXT"} else {"NO CONTEXT"},
            "reason":reason,
            "similarity_floor":self.floor,
            "minimum_term_coverage":MIN_COVERAGE,
            "subject_term":terms.first(),
            "required_identifiers":identifiers,
            "top_term_coverage":evaluations.first().map(|e|e.coverage),
            "candidate_count":rows.len(),
            "admitted_count":accepted.len(),
        });
        Ok(())
    }

    fn evaluate(&self, row: &Value, terms: &[String], identifiers: &[String]) -> Evaluation {
        let similarity = row["similarity"]
            .as_f64()
            .filter(|s| s.is_finite() && (-1.0..=1.0).contains(s));
        let text = format!(
            "{} {}",
            row["entity"].as_str().unwrap_or(""),
            row["text"].as_str().unwrap_or("")
        );
        let words: BTreeSet<_> = words(&text).into_iter().collect();
        let coverage = if terms.is_empty() {
            0.0
        } else {
            terms.iter().filter(|term| words.contains(*term)).count() as f64 / terms.len() as f64
        };
        let reason = match similarity {
            None => "similarity_unavailable",
            Some(s) if s < self.floor => "below_similarity_floor",
            _ if !terms.first().is_some_and(|term| words.contains(term)) => "subject_not_covered",
            _ if !identifiers.iter().all(|id| exact_identifier(&text, id)) => {
                "identifier_not_covered"
            }
            _ if coverage < MIN_COVERAGE => "insufficient_term_coverage",
            _ => "sufficient_context",
        };
        Evaluation { reason, coverage }
    }
}

struct Evaluation {
    reason: &'static str,
    coverage: f64,
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn query_terms(query: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    words(query)
        .into_iter()
        .filter(|s| !STOP.contains(&s.as_str()) && seen.insert(s.clone()))
        .collect()
}

fn identifier_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/')
}

fn identifiers(query: &str) -> Vec<String> {
    query
        .split(|c: char| !identifier_char(c))
        .map(|s| s.trim_matches(|c| matches!(c, '.' | ':' | '/')))
        .filter(|s| {
            !s.is_empty()
                && (s.chars().any(|c| matches!(c, '-' | '_' | '.' | ':' | '/'))
                    || (s.len() >= 7 && s.chars().all(|c| c.is_ascii_hexdigit())))
        })
        .map(str::to_lowercase)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn exact_identifier(text: &str, id: &str) -> bool {
    let text = text.to_lowercase();
    text.match_indices(id).any(|(start, _)| {
        !text[..start]
            .chars()
            .next_back()
            .is_some_and(identifier_char)
            && !text[start + id.len()..]
                .chars()
                .next()
                .is_some_and(identifier_char)
    })
}

#[cfg(test)]
#[path = "search_confidence_tests.rs"]
mod tests;
