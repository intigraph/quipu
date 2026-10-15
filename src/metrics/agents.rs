//! `/query` load by declared agent (aegis-ufw).
//!
//! `X-Quipu-Agent` already reaches the server (seeds `sd` sends the crew name on
//! every read) but was recorded only in the per-request log, which is not
//! readable by the callers who need the answer. This counter makes it a bounded
//! metric. The value is DECLARED by the caller, never authenticated: it names
//! who said they were asking, not who provably did.
//!
//! Cardinality is fixed: `/query` only, keyed by `(client, agent)`, with at most
//! [`MAX_AGENTS`] agents and the same client budget as the other client metrics.
//! No task and no endpoint dimension, so it cannot multiply into an
//! agent x task x endpoint grid. Missing or unusable values are `undeclared`;
//! overflow folds into `other`. Both are real answers, measured, never dropped.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Mutex;

/// Distinct declared agents retained per process before folding into `other`.
pub const MAX_AGENTS: usize = 32;

/// Normalise a declared agent into a bounded label. Same character rules as
/// the client label; absent or empty after cleaning is `undeclared`.
#[must_use]
pub fn normalize_agent(declared: Option<&str>) -> String {
    let cleaned: String = declared
        .unwrap_or("")
        .split(['/', ' ', '\t', '\n', '\r'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .take(32)
        .collect();
    if cleaned.is_empty() {
        "undeclared".to_string()
    } else {
        cleaned
    }
}

#[derive(Default)]
pub(crate) struct AgentQueryMetrics {
    /// (client, agent) -> (completed `/query` requests, summed seconds).
    map: Mutex<BTreeMap<(String, String), (u64, f64)>>,
}

fn bounded<'a>(known: &BTreeSet<&str>, value: &'a str, cap: usize) -> &'a str {
    if known.contains(value) || known.len() < cap - 1 {
        value
    } else {
        "other"
    }
}

impl AgentQueryMetrics {
    /// Count one completed request if it is `/query`; other route templates are
    /// ignored (`quipu_http_client_requests_total` covers them). `client` is
    /// already normalised; `declared_agent` is the raw header value, if any.
    pub(crate) fn observe(
        &self,
        client: &str,
        endpoint: &str,
        declared_agent: Option<&str>,
        seconds: f64,
    ) {
        if endpoint != "/query" {
            return;
        }
        let agent = normalize_agent(declared_agent);
        let mut map = self
            .map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let clients: BTreeSet<&str> = map
            .keys()
            .map(|(c, _)| c.as_str())
            .filter(|c| *c != "other")
            .collect();
        let agents: BTreeSet<&str> = map
            .keys()
            .map(|(_, a)| a.as_str())
            .filter(|a| *a != "other")
            .collect();
        let key = (
            bounded(&clients, client, super::MAX_CLIENTS).to_string(),
            bounded(&agents, &agent, MAX_AGENTS).to_string(),
        );
        let entry = map.entry(key).or_default();
        entry.0 += 1;
        entry.1 += seconds;
    }

    pub(crate) fn render(&self, out: &mut String) {
        let map = self
            .map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        out.push_str(
            "# HELP quipu_http_agent_query_requests_total Completed /query requests by normalised caller and DECLARED (unauthenticated) X-Quipu-Agent.\n\
             # TYPE quipu_http_agent_query_requests_total counter\n",
        );
        for ((client, agent), (n, _)) in map.iter() {
            let _ = writeln!(
                out,
                "quipu_http_agent_query_requests_total{{client=\"{}\",agent=\"{}\"}} {n}",
                super::esc(client),
                super::esc(agent)
            );
        }
        out.push_str(
            "# HELP quipu_http_agent_query_seconds_total Cumulative /query request seconds by normalised caller and DECLARED X-Quipu-Agent.\n\
             # TYPE quipu_http_agent_query_seconds_total counter\n",
        );
        for ((client, agent), (_, secs)) in map.iter() {
            let _ = writeln!(
                out,
                "quipu_http_agent_query_seconds_total{{client=\"{}\",agent=\"{}\"}} {secs}",
                super::esc(client),
                super::esc(agent)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_unusable_agents_are_undeclared() {
        assert_eq!(normalize_agent(None), "undeclared");
        assert_eq!(normalize_agent(Some("  ")), "undeclared");
        assert_eq!(normalize_agent(Some("\"}\n")), "undeclared");
        assert_eq!(normalize_agent(Some("Franklin")), "franklin");
        assert_eq!(normalize_agent(Some("tim/session 7")), "tim");
        assert_eq!(normalize_agent(Some(&"x".repeat(80))).len(), 32);
    }

    #[test]
    fn agents_fold_into_other_at_the_cap_and_existing_ones_keep_counting() {
        let m = AgentQueryMetrics::default();
        for i in 0..MAX_AGENTS + 10 {
            m.observe("seeds", "/query", Some(&format!("crew{i}")), 0.5);
        }
        m.observe("seeds", "/query", Some("crew0"), 0.5);
        m.observe("seeds", "/query", None, 0.25);
        let map = m.map.lock().unwrap();
        let agents: BTreeSet<&str> = map.keys().map(|(_, a)| a.as_str()).collect();
        assert!(agents.len() <= MAX_AGENTS, "{}", agents.len());
        assert_eq!(map[&("seeds".into(), "crew0".into())].0, 2);
        // crew0..crew30 named, then everything else (incl. undeclared, which came late) is other.
        assert_eq!(map[&("seeds".into(), "other".into())].0, 12);
    }

    #[test]
    fn render_names_both_series_and_escapes_nothing_unsafe() {
        let m = AgentQueryMetrics::default();
        m.observe("seeds", "/query", Some("tim"), 1.5);
        m.observe("seeds", "/knot", Some("tim"), 9.0);
        m.observe("seeds", "/query", None, 0.5);
        let mut out = String::new();
        m.render(&mut out);
        assert!(
            out.contains(
                "quipu_http_agent_query_requests_total{client=\"seeds\",agent=\"tim\"} 1\n"
            )
        );
        assert!(out.contains(
            "quipu_http_agent_query_requests_total{client=\"seeds\",agent=\"undeclared\"} 1\n"
        ));
        assert!(out.contains(
            "quipu_http_agent_query_seconds_total{client=\"seeds\",agent=\"tim\"} 1.5\n"
        ));
        assert!(out.contains("DECLARED"));
    }
}
