//! Episode SHACL admission and its existing post-refusal audit record.
//!
//! Keep structured shape metadata before the public error becomes prose.
use crate::{Error, Result, Store};

use super::Episode;

pub(super) fn validate_or_refuse(
    store: &Store,
    episode: &Episode,
    shapes: &str,
    data: &str,
    timestamp: &str,
) -> Result<()> {
    let feedback = crate::shacl_context::validate_with_store_context(store, shapes, data)?;
    if feedback.conforms {
        return Ok(());
    }
    let messages: Vec<_> = feedback
        .results
        .iter()
        .map(|r| {
            format!(
                "{}: {} ({})",
                r.severity,
                r.message.as_deref().unwrap_or("no message"),
                r.focus_node
            )
        })
        .collect();
    let refusal = crate::store::events::PendingRefusal {
        gate: "shacl",
        reason_code: "shacl",
        shape: crate::metrics::refusals::shape_id(
            feedback
                .results
                .iter()
                .filter(|r| r.severity.eq_ignore_ascii_case("violation"))
                .map(|r| r.source_shape.as_deref()),
        ),
        reason: format!(
            "SHACL validation failed: {} violation(s): {}",
            feedback.violations,
            messages
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        ),
        refused_datums: episode.nodes.len() + episode.edges.len(),
    };
    // As before: before the transaction; operational validation errors above
    // are not gate verdicts. No body is recorded and the original error survives.
    store.record_refusal(
        &refusal,
        episode
            .graph
            .as_deref()
            .filter(|g| !g.trim().is_empty())
            .unwrap_or(crate::schema::ROOT_GRAPH_IRI),
        episode.source.as_deref(),
        Some(&format!("episode:{}", episode.name)),
        timestamp,
    );
    Err(Error::ValidationFailed {
        violations: feedback.violations,
        messages,
    })
}
