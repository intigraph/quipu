//! Bounded process-local refusal counters. Durable history is write.refused.
use std::collections::BTreeMap;
#[cfg(feature = "shacl")]
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::Mutex;

use serde_json::{Value, json};

const MAX_SERIES: usize = 1024;
const REASONS: [&str; 9] = [
    "shacl",
    "policy",
    "authority",
    "owl",
    "placement",
    "cannot_verify",
    "operational_failure",
    "conflict",
    "other",
];
type Key = (&'static str, String, String, String);

#[derive(Default)]
struct State {
    counts: BTreeMap<Key, u64>,
    event_failures: u64,
}

#[derive(Default)]
pub(super) struct RefusalMetrics(Mutex<State>);

fn label(value: &str, fallback: &str) -> String {
    let value: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect();
    if value.trim().is_empty() {
        fallback.into()
    } else {
        value
    }
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

impl RefusalMetrics {
    pub(super) fn observe(
        &self,
        reason: &str,
        shape: Option<&str>,
        graph: &str,
        client: &str,
        event_recorded: bool,
    ) {
        let reason = REASONS
            .iter()
            .copied()
            .find(|r| *r == reason)
            .unwrap_or("other");
        let mut key = (
            reason,
            label(shape.unwrap_or("unknown"), "unknown"),
            label(graph, "unknown"),
            super::normalize_client(Some(client), None),
        );
        let mut state = self.0.lock().unwrap();
        // Reserve one overflow bucket per bounded reason, preserving all counts.
        if !state.counts.contains_key(&key) && state.counts.len() >= MAX_SERIES - REASONS.len() {
            key = (reason, "other".into(), "other".into(), "other".into());
        }
        *state.counts.entry(key).or_default() += 1;
        state.event_failures += u64::from(!event_recorded);
    }

    pub(super) fn snapshot(&self, since: Option<f64>) -> Value {
        let state = self.0.lock().unwrap();
        let mut by_reason = BTreeMap::new();
        let mut series = Vec::with_capacity(state.counts.len());
        for ((reason, shape, graph, client), count) in &state.counts {
            *by_reason.entry(*reason).or_insert(0_u64) += count;
            series.push(json!({
                "reason": reason, "shape": shape, "graph": graph, "client": client, "count": count,
            }));
        }
        json!({
            "total": state.counts.values().sum::<u64>(),
            "by_reason": by_reason, "series": series, "series_limit": MAX_SERIES,
            "event_record_failures": state.event_failures,
            "counter_scope": "process", "reset_on_restart": true, "since_unix_seconds": since,
        })
    }

    pub(super) fn render(&self, out: &mut String) {
        let state = self.0.lock().unwrap();
        out.push_str("# HELP quipu_write_refusals_total Gate-refused write attempts, counted once after rollback. Process-local; resets on restart. Unknown shape metadata is explicit.\n# TYPE quipu_write_refusals_total counter\n");
        for ((reason, shape, graph, client), n) in &state.counts {
            let _ = writeln!(
                out,
                "quipu_write_refusals_total{{reason=\"{reason}\",shape=\"{}\",graph=\"{}\",client=\"{}\"}} {n}",
                escape(shape),
                escape(graph),
                escape(client)
            );
        }
        out.push_str("# HELP quipu_write_refusal_event_failures_total Refusals whose audit event could not be recorded. Process-local.\n# TYPE quipu_write_refusal_event_failures_total counter\n");
        let _ = writeln!(
            out,
            "quipu_write_refusal_event_failures_total {}",
            state.event_failures
        );
    }
}

/// Retain only structured validator shape identifiers, never parse error prose.
#[cfg(feature = "shacl")]
pub(crate) fn shape_id<'a>(shapes_in: impl IntoIterator<Item = Option<&'a str>>) -> Option<String> {
    let mut shapes = BTreeSet::new();
    for shape in shapes_in.into_iter().flatten() {
        let shape = shape.trim().trim_matches(['<', '>']);
        if shape.is_empty() {
            continue;
        }
        shapes.insert(if shape.starts_with("_:") {
            "anonymous"
        } else {
            shape
        });
        if shapes.len() > 1 {
            return Some("multiple".into());
        }
    }
    match shapes.len() {
        0 => None,
        1 => shapes.into_iter().next().map(str::to_owned),
        _ => Some("multiple".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_and_exposition_count_writes_not_violations_and_keep_unknowns() {
        let counters = RefusalMetrics::default();
        assert_eq!(counters.snapshot(None)["total"], 0);
        counters.observe(
            "shacl",
            Some("https://example.test/shape"),
            "urn:graph",
            "Test/1.0",
            true,
        );
        counters.observe("policy", None, "urn:graph", "", false);
        let stats = counters.snapshot(Some(100.0));
        assert_eq!(stats["total"], 2);
        assert_eq!(stats["by_reason"]["shacl"], 1);
        assert_eq!(stats["by_reason"]["policy"], 1);
        assert_eq!(stats["event_record_failures"], 1);
        assert_eq!(stats["reset_on_restart"], true);
        let mut text = String::new();
        counters.render(&mut text);
        assert!(text.contains("reason=\"shacl\",shape=\"https://example.test/shape\",graph=\"urn:graph\",client=\"test\"} 1"));
        assert!(text.contains("shape=\"unknown\",graph=\"urn:graph\",client=\"unattributed\"} 1"));
        assert!(text.contains("quipu_write_refusal_event_failures_total 1"));
    }

    #[test]
    fn arbitrary_labels_have_a_finite_series_budget_without_losing_counts() {
        let counters = RefusalMetrics::default();
        for n in 0..2000 {
            counters.observe(
                "shacl",
                Some(&format!("urn:shape:{n}")),
                &format!("urn:graph:{n}"),
                &format!("client{n}"),
                true,
            );
        }
        for reason in REASONS {
            counters.observe(reason, None, "new", "new", true);
        }
        let stats = counters.snapshot(None);
        assert!(stats["series"].as_array().unwrap().len() <= MAX_SERIES);
        assert_eq!(stats["total"], 2009);
        assert_eq!(stats["by_reason"]["shacl"], 2001);
        assert!(
            stats["series"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["graph"] == "other")
        );
    }

    #[test]
    fn untrusted_labels_cannot_inject_prometheus_lines() {
        let counters = RefusalMetrics::default();
        counters.observe(
            "arbitrary\nreason",
            Some("urn:s\"\\\nmetric 99"),
            "urn:g\r\n",
            "client\n",
            true,
        );
        let mut text = String::new();
        counters.render(&mut text);
        assert!(!text.lines().any(|s| s.starts_with("metric")));
        assert!(text.contains("reason=\"other\""));
        assert!(text.contains("shape=\"urn:s\\\"\\\\metric 99\""));
    }

    #[cfg(feature = "shacl")]
    #[test]
    fn shape_identity_is_structured_and_does_not_mint_blank_node_series() {
        assert_eq!(shape_id([None]), None);
        assert_eq!(
            shape_id([Some("<urn:shape>"), Some("urn:shape")]),
            Some("urn:shape".into())
        );
        assert_eq!(
            shape_id([Some("_:one"), Some("_:two")]),
            Some("anonymous".into())
        );
        assert_eq!(
            shape_id([Some("urn:one"), Some("urn:two")]),
            Some("multiple".into())
        );
    }
}
