//! Semantic counters use the canonical terminal event, never HTTP cardinality.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Mutex;

use crate::search_outcome::observation::{Event, Surface};
use crate::search_outcome::{Operation, Outcome, ZeroKind};

#[derive(Default)]
struct Counts {
    requests: BTreeMap<(Surface, Operation, Outcome), u64>,
    zeros: BTreeMap<(Surface, Operation, ZeroKind), u64>,
    reasons: BTreeMap<(Surface, Operation, &'static str), u64>,
}

#[derive(Default)]
pub(super) struct SearchMetrics(Mutex<Counts>);

impl SearchMetrics {
    pub(super) fn observe(&self, event: &Event) {
        let mut counts = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let summary = event.summary;
        *counts
            .requests
            .entry((event.surface, event.operation, summary.outcome))
            .or_default() += 1;
        if matches!(summary.outcome, Outcome::Empty | Outcome::NoContext) {
            *counts
                .reasons
                .entry((event.surface, event.operation, miss_reason(event)))
                .or_default() += 1;
        }
        if let Some(kind) = summary.zero_kind {
            // Defend the public event boundary too: an externally constructed
            // summary cannot turn missing metadata or failure into a zero.
            let valid = match kind {
                ZeroKind::NoCandidates => {
                    summary.outcome == Outcome::Empty && summary.candidate_count == Some(0)
                }
                ZeroKind::AllRejected => {
                    summary.outcome == Outcome::NoContext
                        && summary.candidate_count.is_some_and(|n| n > 0)
                        && summary.admitted_count == Some(0)
                }
                ZeroKind::EmptyRows => {
                    summary.outcome == Outcome::Empty && summary.result_count == Some(0)
                }
            };
            if valid {
                *counts
                    .zeros
                    .entry((event.surface, event.operation, kind))
                    .or_default() += 1;
            }
        }
    }

    pub(super) fn render(&self, out: &mut String) {
        out.push_str("# HELP quipu_search_requests_total Completed logical retrieval calls by validated semantic outcome.\n# TYPE quipu_search_requests_total counter\n");
        let counts = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // These labels are serialized fixed enums, never caller-supplied text.
        for ((surface, operation, outcome), count) in &counts.requests {
            let _ = writeln!(
                out,
                "quipu_search_requests_total{{surface={},operation={},outcome={}}} {count}",
                label(surface),
                label(operation),
                label(outcome)
            );
        }
        out.push_str("# HELP quipu_search_zero_results_total Validated successful empty retrievals; kinds are mutually exclusive.\n# TYPE quipu_search_zero_results_total counter\n");
        for ((surface, operation, kind), count) in &counts.zeros {
            let _ = writeln!(
                out,
                "quipu_search_zero_results_total{{surface={},operation={},kind={}}} {count}",
                label(surface),
                label(operation),
                label(kind)
            );
        }
        out.push_str("# HELP quipu_search_miss_reasons_total Semantic misses by a fixed reason vocabulary.\n# TYPE quipu_search_miss_reasons_total counter\n");
        for ((surface, operation, reason), count) in &counts.reasons {
            let _ = writeln!(
                out,
                "quipu_search_miss_reasons_total{{surface={},operation={},reason={}}} {count}",
                label(surface),
                label(operation),
                label(reason)
            );
        }
    }
}

fn miss_reason(event: &Event) -> &'static str {
    match event.summary.zero_kind {
        Some(ZeroKind::NoCandidates) => return "no_candidates",
        Some(ZeroKind::EmptyRows) => return "empty_rows",
        _ => (),
    }
    match event
        .details
        .as_ref()
        .and_then(|details| details.context_reason.as_deref())
    {
        Some("no_result_above_floor") => "no_result_above_floor",
        Some("top_result_below_floor") => "top_result_below_floor",
        Some("identifier_not_covered") => "identifier_not_covered",
        Some("insufficient_term_coverage") => "insufficient_term_coverage",
        Some("subject_not_covered") => "subject_not_covered",
        Some("query_too_long") => "query_too_long",
        Some("query_terms_unavailable") => "query_terms_unavailable",
        Some(_) => "other",
        None => "unknown",
    }
}

fn label(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap()
}

#[cfg(test)]
mod tests;
