//! `/query` load by DECLARED operation (aegis-hg6).
//!
//! `X-Quipu-Operation` already reaches the completion log as
//! `declared_operation`, but a log line is not something a Prometheus probe can
//! sum. Push consumers read on fixed cadences that cannot be told apart from
//! reads caused by a pushed change; this counter lets a caller say which one a
//! read is, and lets the probe count it.
//!
//! The label is bounded by a server-owned allowlist of GENERIC values
//! ([`OPERATIONS`]). Quipu gives no consumer a name here. An absent header is
//! `unlabelled`; a present header that is repeated, malformed or not on the
//! allowlist is `other`. Both are real answers, measured, never dropped. The
//! value is DECLARED, never authenticated, and never authorization.
//!
//! Cardinality is fixed: `/query` only, keyed by `(client, operation)`, with
//! the shared client budget and at most `OPERATIONS.len() + 2` operations.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Mutex;

/// Operations counted under their own name. Everything else declared is `other`.
///
/// The first group names why a read happened; the second names the generic
/// read verb an SDK call performs. Extend deliberately: each value is a series
/// per client.
pub const OPERATIONS: &[&str] = &[
    // Read causes.
    "change-follow",
    "poll",
    "reconcile",
    "canary",
    "backfill",
    "startup",
    "interactive",
    // Generic read verbs.
    "query",
    "show",
    "list",
    "ready",
    "search",
    "count",
    "stats",
    "health",
];

/// The label for a request with no `X-Quipu-Operation` header at all.
pub const UNLABELLED: &str = "unlabelled";
/// The label for a declared operation that is ambiguous or not on [`OPERATIONS`].
pub const OTHER: &str = "other";

/// Map every `X-Quipu-Operation` header value of one request (raw bytes, in
/// order) to a bounded label.
///
/// Exactly one value, matching an allowlisted operation exactly, counts as
/// itself. Repeats are ambiguous and are never resolved by picking one.
#[must_use]
pub fn operation_label<'a>(values: impl IntoIterator<Item = &'a [u8]>) -> &'static str {
    let mut values = values.into_iter();
    let Some(first) = values.next() else {
        return UNLABELLED;
    };
    if values.next().is_some() {
        return OTHER;
    }
    OPERATIONS
        .iter()
        .find(|known| known.as_bytes() == first)
        .copied()
        .unwrap_or(OTHER)
}

/// (client, operation) -> (completed `/query` requests, summed seconds).
type Counts = BTreeMap<(String, &'static str), (u64, f64)>;

#[derive(Default)]
pub(crate) struct OperationQueryMetrics {
    map: Mutex<Counts>,
}

impl OperationQueryMetrics {
    /// Count one completed request if it is `/query`. `client` is already
    /// normalised; `operation` comes from [`operation_label`].
    pub(crate) fn observe(&self, client: &str, endpoint: &str, operation: &'static str, secs: f64) {
        if endpoint != "/query" {
            return;
        }
        let mut map = self
            .map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let clients: BTreeSet<&str> = map
            .keys()
            .map(|(c, _)| c.as_str())
            .filter(|c| *c != "other")
            .collect();
        let client = if clients.contains(client) || clients.len() < super::MAX_CLIENTS - 1 {
            client
        } else {
            "other"
        };
        let entry = map.entry((client.to_string(), operation)).or_default();
        entry.0 += 1;
        entry.1 += secs;
    }

    pub(crate) fn render(&self, out: &mut String) {
        let map = self
            .map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        out.push_str(
            "# HELP quipu_http_operation_query_requests_total Completed /query requests by normalised caller and DECLARED (unauthenticated) X-Quipu-Operation.\n\
             # TYPE quipu_http_operation_query_requests_total counter\n",
        );
        for ((client, operation), (n, _)) in map.iter() {
            let _ = writeln!(
                out,
                "quipu_http_operation_query_requests_total{{client=\"{}\",operation=\"{operation}\"}} {n}",
                super::esc(client),
            );
        }
        out.push_str(
            "# HELP quipu_http_operation_query_seconds_total Cumulative /query request seconds by normalised caller and DECLARED X-Quipu-Operation.\n\
             # TYPE quipu_http_operation_query_seconds_total counter\n",
        );
        for ((client, operation), (_, secs)) in map.iter() {
            let _ = writeln!(
                out,
                "quipu_http_operation_query_seconds_total{{client=\"{}\",operation=\"{operation}\"}} {secs}",
                super::esc(client),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(values: &[&str]) -> &'static str {
        operation_label(values.iter().map(|v| v.as_bytes()))
    }

    #[test]
    fn absent_is_unlabelled_and_unknown_or_ambiguous_is_other() {
        assert_eq!(label(&[]), UNLABELLED);
        assert_eq!(label(&["change-follow"]), "change-follow");
        assert_eq!(label(&["poll"]), "poll");
        assert_eq!(label(&["Poll"]), OTHER);
        assert_eq!(label(&["poll "]), OTHER);
        assert_eq!(label(&["free-form-name"]), OTHER);
        assert_eq!(label(&[""]), OTHER);
        assert_eq!(label(&["poll", "poll"]), OTHER);
        assert_eq!(label(&["poll", "canary"]), OTHER);
    }

    #[test]
    fn allowlist_is_generic_bounded_and_cannot_collide_with_fallbacks() {
        let set: BTreeSet<&str> = OPERATIONS.iter().copied().collect();
        assert_eq!(set.len(), OPERATIONS.len(), "duplicate operation");
        assert!(!set.contains(UNLABELLED) && !set.contains(OTHER));
        assert!(
            OPERATIONS
                .iter()
                .all(|op| op.chars().all(|c| c.is_ascii_lowercase() || c == '-'))
        );
    }

    #[test]
    fn only_query_is_counted_and_unbounded_declarations_stay_bounded() {
        let m = OperationQueryMetrics::default();
        for i in 0..200 {
            let declared = format!("op{i}");
            m.observe("pusher", "/query", label(&[&declared]), 0.1);
        }
        m.observe("pusher", "/knot", "poll", 9.0);
        let map = m.map.lock().unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map[&("pusher".into(), OTHER)].0, 200);
    }

    #[test]
    fn clients_fold_into_other_at_the_shared_cap() {
        let m = OperationQueryMetrics::default();
        for i in 0..super::super::MAX_CLIENTS + 5 {
            m.observe(&format!("c{i}"), "/query", "poll", 0.1);
        }
        let map = m.map.lock().unwrap();
        assert_eq!(map.len(), super::super::MAX_CLIENTS);
        assert_eq!(map[&("other".into(), "poll")].0, 6);
    }

    #[test]
    fn render_names_both_series() {
        let m = OperationQueryMetrics::default();
        m.observe("chaski-adapter", "/query", "change-follow", 1.5);
        m.observe("chaski-adapter", "/query", UNLABELLED, 0.5);
        let mut out = String::new();
        m.render(&mut out);
        assert!(out.contains(
            "quipu_http_operation_query_requests_total{client=\"chaski-adapter\",operation=\"change-follow\"} 1\n"
        ));
        assert!(out.contains(
            "quipu_http_operation_query_requests_total{client=\"chaski-adapter\",operation=\"unlabelled\"} 1\n"
        ));
        assert!(out.contains(
            "quipu_http_operation_query_seconds_total{client=\"chaski-adapter\",operation=\"change-follow\"} 1.5\n"
        ));
    }
}
