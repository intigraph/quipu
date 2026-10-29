//! Semantic retrieval outcomes, independent of transport and instrumentation.
//!
//! Unknown metadata is never an empty answer. Callers must classify transport
//! failures before passing a successful response to [`classify`]. This module
//! does not change ranking, admission, query execution, or response bodies.

use serde::Serialize;
use serde_json::Value;

/// Bounded operation vocabulary shared by event records and metric labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// Search with the versioned context-admission contract.
    Search,
    /// Compatibility/vector search with no context-admission contract.
    HybridSearch,
    /// Compatibility node search.
    SearchNodes,
    /// Compatibility fact search.
    SearchFacts,
    /// Search-rooted SPARQL retrieval.
    SearchQuery,
    /// SPARQL retrieval.
    Query,
    /// Stored-query retrieval (not the SPARQL ASK form).
    NamedQuery,
}

/// Bounded logical outcome; failures never increment a zero-result counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The operation returned a validated nonempty answer.
    Hits,
    /// A validated successful retrieval found no candidates or rows.
    Empty,
    /// Candidates existed but context admission rejected them all.
    NoContext,
    /// Execution failed.
    Error,
    /// Execution exceeded its deadline.
    Timeout,
    /// Execution was refused.
    Refused,
    /// Evidence is missing, contradictory, or malformed.
    Unknown,
}

/// Mutually exclusive zero-result kinds. Not every unknown is a zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ZeroKind {
    /// The search returned a validated empty candidate array.
    NoCandidates,
    /// The context contract explicitly rejected all existing candidates.
    AllRejected,
    /// A successful query returned an empty row/triple relation.
    EmptyRows,
}

pub mod evidence;
mod journal;
/// Logical-call scope and terminal accounting, shared by adapters and core.
pub mod observation;
pub(crate) use journal::render_health as render_capture_health;

/// Nullable cardinalities preserve which stage was actually observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// Logical outcome, separate from HTTP success.
    pub outcome: Outcome,
    /// Present only for a proven zero-result kind.
    pub zero_kind: Option<ZeroKind>,
    /// Raw candidates before context admission, when observed.
    pub candidate_count: Option<usize>,
    /// Admitted context, only with a validated versioned contract.
    pub admitted_count: Option<usize>,
    /// Returned query rows/triples, or one boolean ASK answer.
    pub result_count: Option<usize>,
}

impl Summary {
    /// Cardinality from an actual typed query result, without JSON guessing.
    #[must_use]
    pub fn query_result(result: &crate::QueryResult) -> Self {
        let n = match result {
            crate::QueryResult::Select { rows, .. } => rows.len(),
            crate::QueryResult::Graph(triples) => triples.len(),
            crate::QueryResult::Ask(_) => 1,
        };
        Self {
            outcome: if n == 0 {
                Outcome::Empty
            } else {
                Outcome::Hits
            },
            zero_kind: (n == 0).then_some(ZeroKind::EmptyRows),
            candidate_count: None,
            admitted_count: None,
            result_count: Some(n),
        }
    }
    /// A failure or unknown without invented cardinalities.
    #[must_use]
    pub const fn unavailable(outcome: Outcome) -> Self {
        Self {
            outcome,
            zero_kind: None,
            candidate_count: None,
            admitted_count: None,
            result_count: None,
        }
    }
}

fn count(value: &Value, key: &str) -> Option<usize> {
    usize::try_from(value.get(key)?.as_u64()?).ok()
}

fn objects(value: &Value, key: &str) -> Option<usize> {
    let rows = value.get(key)?.as_array()?;
    // A valid empty relation is authoritative; a null/scalar element is not.
    if !rows.iter().all(Value::is_object) {
        return None;
    }
    Some(rows.len())
}

fn relation(value: &Value, key: &str) -> Option<usize> {
    let size = objects(value, key)?;
    if value.get("count").is_some() && count(value, "count") != Some(size) {
        return None;
    }
    Some(size)
}

fn search(value: &Value) -> Summary {
    let mut out = Summary::unavailable(Outcome::Unknown);
    let Some(candidates) = relation(value, "results") else {
        return out;
    };
    out.candidate_count = Some(candidates);
    let Some(context) = value.get("context").filter(|c| c.is_object()) else {
        return out;
    };
    let Some(admitted) = objects(value, "context_results") else {
        return out;
    };
    // The envelope's public count is not reused as a context count. Admission
    // counts must agree with their own arrays and their own versioned contract.
    if context.get("contract").and_then(Value::as_str) != Some("quipu-context/v1")
        || count(context, "candidate_count") != Some(candidates)
        || count(context, "admitted_count") != Some(admitted)
        || admitted > candidates
        || context
            .get("reason")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || !value["context_results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| value["results"].as_array().unwrap().contains(row))
    {
        return out;
    }
    let positive = context.get("outcome").and_then(Value::as_str) == Some("CONTEXT");
    let negative = context.get("outcome").and_then(Value::as_str) == Some("NO CONTEXT");
    if value.get("insufficient_context").and_then(Value::as_bool) != Some(negative)
        || !(positive && admitted > 0 || negative && admitted == 0)
    {
        return out;
    }
    out.admitted_count = Some(admitted);
    if positive {
        out.outcome = Outcome::Hits;
    } else if candidates == 0 {
        out.outcome = Outcome::Empty;
        out.zero_kind = Some(ZeroKind::NoCandidates);
    } else {
        out.outcome = Outcome::NoContext;
        out.zero_kind = Some(ZeroKind::AllRejected);
    }
    out
}

/// Classify a successful response without treating absent fields as zero.
///
/// Transport errors, deadlines and refusals must use [`Summary::unavailable`]
/// instead. An error-shaped body is unknown even when its transport succeeded.
#[must_use]
pub fn classify(operation: Operation, value: &Value) -> Summary {
    let mut out = Summary::unavailable(Outcome::Unknown);
    if !value.is_object()
        || value.get("error").is_some()
        || value.get("isError") == Some(&Value::Bool(true))
    {
        return out;
    }
    if operation == Operation::Search {
        return search(value);
    }
    if operation == Operation::SearchQuery {
        let Some(result) = value.get("result").filter(|v| v.is_object()) else {
            return out;
        };
        out = classify(Operation::Query, result);
        out.candidate_count = count(value, "candidate_count");
        return out;
    }
    if matches!(operation, Operation::Query | Operation::NamedQuery) {
        // ASK false is one answered boolean, never an empty relation. Refuse
        // ambiguous envelopes instead of choosing a convenient representation.
        if let Some(answer) = value.get("result") {
            if answer.is_boolean() && value.get("rows").is_none() && value.get("triples").is_none()
            {
                out.outcome = Outcome::Hits;
                out.result_count = Some(1);
            }
            return out;
        }
        let key = match (value.get("rows"), value.get("triples")) {
            (Some(_), None) => "rows",
            (None, Some(_)) => "triples",
            _ => return out,
        };
        if let Some(n) = relation(value, key) {
            out.result_count = Some(n);
            out.outcome = if n == 0 {
                Outcome::Empty
            } else {
                Outcome::Hits
            };
            out.zero_kind = (n == 0).then_some(ZeroKind::EmptyRows);
        }
        return out;
    }
    let key = match operation {
        Operation::HybridSearch => "results",
        Operation::SearchNodes => "nodes",
        Operation::SearchFacts => "facts",
        _ => return out,
    };
    if let Some(n) = relation(value, key) {
        out.candidate_count = Some(n);
        out.outcome = if n == 0 {
            Outcome::Empty
        } else {
            Outcome::Hits
        };
        out.zero_kind = (n == 0).then_some(ZeroKind::NoCandidates);
    }
    out
}

#[cfg(test)]
mod tests;
