//! Carry one retrieval scope from the HTTP adapter into blocking core work.

pub(super) use quipu::search_outcome::observation::{Call, Surface};
use quipu::search_outcome::{Outcome, Summary, ZeroKind};
use std::sync::Arc;

tokio::task_local! {
    pub(super) static REQUEST: Option<Arc<Call>>;
}

pub(super) fn current() -> Option<Arc<Call>> {
    REQUEST.try_with(Clone::clone).ok().flatten()
}

pub(super) fn input(value: &serde_json::Value) {
    if let Some(call) = current() {
        call.input(value);
    }
}

pub(super) fn start(route: &str, surface: Surface) -> Option<Arc<Call>> {
    quipu::search_outcome::observation::for_route(route)
        .map(|operation| Call::new(surface, operation))
}

pub(super) fn finish(call: &Call, status: u16, usage: Option<quipu::request_usage::RequestUsage>) {
    // W3C/protocol queries need not pass through the JSON tool wrapper. These
    // annotations originate from actual typed query results, unlike a guessed
    // count from an arbitrary successful JSON response.
    let fallback = usage.and_then(|usage| {
        use quipu::request_usage::QueryShape;
        if !matches!(
            usage.query_shape,
            QueryShape::Select | QueryShape::Construct | QueryShape::Describe | QueryShape::Ask
        ) {
            return None;
        }
        let n = usage.result_size?;
        if usage.query_shape == QueryShape::Ask && n != 1 {
            return None;
        }
        let empty = n == 0 && usage.query_shape != QueryShape::Ask;
        Some(Summary {
            outcome: if empty { Outcome::Empty } else { Outcome::Hits },
            zero_kind: empty.then_some(ZeroKind::EmptyRows),
            candidate_count: None,
            admitted_count: None,
            result_count: Some(n),
        })
    });
    if call.is_mcp() {
        call.transport_finished(status, fallback);
    } else if let Some(event) = call.finish(Some(status), fallback) {
        quipu::search_outcome::observation::emit(&event);
    }
}

#[cfg(test)]
#[path = "search_observation_tests.rs"]
mod tests;
