use super::*;
use crate::search_outcome::observation::Call;
use crate::search_outcome::{Summary, classify};
use serde_json::json;

#[test]
fn unknown_reason_text_never_becomes_a_metric_label() {
    let metrics = SearchMetrics::default();
    let mut event = event(Summary {
        outcome: Outcome::NoContext,
        zero_kind: Some(ZeroKind::AllRejected),
        candidate_count: Some(1),
        admitted_count: Some(0),
        result_count: None,
    });
    event.details = Some(crate::search_outcome::evidence::Details {
        context_reason: Some("private-reason-sentinel".into()),
        ..Default::default()
    });
    metrics.observe(&event);
    let mut text = String::new();
    metrics.render(&mut text);
    assert!(text.contains("reason=\"other\"} 1"));
    assert!(!text.contains("private-reason-sentinel"));
}

fn event(summary: Summary) -> Event {
    Call::new(Surface::Http, Operation::Query)
        .finish(Some(200), Some(summary))
        .unwrap()
}

#[test]
fn failures_and_missing_metadata_never_increment_a_zero_counter() {
    let metrics = SearchMetrics::default();
    for outcome in [
        Outcome::Unknown,
        Outcome::Error,
        Outcome::Timeout,
        Outcome::Refused,
    ] {
        metrics.observe(&event(Summary::unavailable(outcome)));
    }
    let mut text = String::new();
    metrics.render(&mut text);
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("quipu_search_requests_total{"))
            .count(),
        4
    );
    assert!(
        !text
            .lines()
            .any(|line| line.starts_with("quipu_search_zero_results_total{"))
    );
    assert!(!text.contains("cli_local")); // Unobserved surface is absent, not zero.
}

#[test]
fn positive_and_genuine_empty_controls_use_one_disjoint_zero_kind() {
    let metrics = SearchMetrics::default();
    metrics.observe(&event(classify(Operation::Query, &json!({"result":false}))));
    metrics.observe(&event(classify(
        Operation::Query,
        &json!({"rows":[],"count":0}),
    )));
    let mut text = String::new();
    metrics.render(&mut text);
    assert!(text.contains("outcome=\"hits\"} 1"));
    assert!(text.contains("outcome=\"empty\"} 1"));
    assert!(text.contains("kind=\"empty_rows\"} 1"));
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("quipu_search_zero_results_total{"))
            .count(),
        1
    );
    assert!(!text.contains("logical_call_id"));
    assert!(!text.contains("query_sha256"));
}

#[test]
fn malformed_external_summaries_cannot_fabricate_a_zero() {
    let metrics = SearchMetrics::default();
    let mut summary = Summary::unavailable(Outcome::Error);
    summary.zero_kind = Some(ZeroKind::EmptyRows);
    summary.result_count = Some(0);
    metrics.observe(&event(summary));
    let mut text = String::new();
    metrics.render(&mut text);
    assert!(
        !text
            .lines()
            .any(|line| line.starts_with("quipu_search_zero_results_total{"))
    );
}
