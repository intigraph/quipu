//! Client-declared request labels for the completion log (aegis-z29i).
//!
//! Any client may declare `x-quipu-source` and `x-quipu-operation`. Quipu
//! checks the syntax only ([`quipu::request_usage::request_label`]) and gives
//! no particular client or value a meaning in source: each client owns its own
//! label vocabulary, and a deployment may name labels in its own configuration
//! (`heavy_read_operations`). Cardinality is bounded generically by
//! `declared_label_limit`. Never authorization, never a metric label.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock, PoisonError};

/// Logged in place of a new label once a field already holds the configured
/// number of distinct labels. It cannot collide with a client declaration:
/// a declared label must start with a lowercase letter.
pub(super) const OVERFLOW: &str = "_overflow";

#[derive(Clone, Default)]
pub(super) struct CommandContext {
    source: Option<String>,
    operation: Option<String>,
}

/// Distinct labels admitted to the log per field, for this process's lifetime.
/// A new label past `limit` is logged as [`OVERFLOW`]; labels already seen
/// keep logging as themselves.
pub(super) struct LabelCap {
    limit: usize,
    seen: Mutex<[HashSet<String>; 2]>,
}

impl LabelCap {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            limit,
            seen: Mutex::new(Default::default()),
        }
    }

    fn admit(&self, field: usize, label: &str) -> String {
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let set = &mut seen[field];
        if set.contains(label) {
            return label.to_owned();
        }
        if set.len() < self.limit {
            set.insert(label.to_owned());
            return label.to_owned();
        }
        OVERFLOW.to_owned()
    }
}

static CAP: OnceLock<LabelCap> = OnceLock::new();

/// Install the deployment's label cap. Later calls are ignored.
pub(crate) fn init(config: &quipu::config::ServerConfig) {
    let _ = CAP.set(LabelCap::new(config.declared_label_limit));
}

fn cap() -> &'static LabelCap {
    CAP.get_or_init(|| LabelCap::new(quipu::config::ServerConfig::default().declared_label_limit))
}

impl CommandContext {
    pub(super) fn operation(&self) -> Option<&str> {
        self.operation.as_deref()
    }

    pub(super) fn from_headers(headers: &axum::http::HeaderMap) -> Self {
        // Exactly one well-formed value, or UNKNOWN. A repeated header is
        // ambiguous and never resolved by picking one.
        let one = |name: &str| {
            let mut values = headers.get_all(name).iter();
            let value = values.next()?.to_str().ok()?;
            values
                .next()
                .is_none()
                .then_some(value)
                .filter(|value| quipu::request_usage::request_label(value))
        };
        Self {
            source: one("x-quipu-source").map(str::to_owned),
            operation: one("x-quipu-operation").map(str::to_owned),
        }
    }

    pub(super) fn annotate(&self, log: String) -> String {
        self.annotate_with(cap(), log)
    }

    fn annotate_with(&self, cap: &LabelCap, log: String) -> String {
        let Ok(serde_json::Value::Object(mut record)) = serde_json::from_str(&log) else {
            return log;
        };
        // Null is UNKNOWN. Do not guess a command from the endpoint or agent.
        let source = self.source.as_deref().map(|label| cap.admit(0, label));
        let operation = self.operation.as_deref().map(|label| cap.admit(1, label));
        record.insert("declared_source".into(), serde_json::json!(source));
        record.insert("declared_operation".into(), serde_json::json!(operation));
        serde_json::Value::Object(record).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logged(source: Option<&str>, operation: Option<&str>) -> serde_json::Value {
        let mut headers = axum::http::HeaderMap::new();
        if let Some(source) = source {
            headers.insert("x-quipu-source", source.parse().unwrap());
        }
        if let Some(operation) = operation {
            headers.insert("x-quipu-operation", operation.parse().unwrap());
        }
        let context = CommandContext::from_headers(&headers);
        serde_json::from_str(&context.annotate_with(
            &LabelCap::new(16),
            "{\"client\":\"example\",\"endpoint\":\"/query\",\"status\":200}".into(),
        ))
        .unwrap()
    }

    #[test]
    fn scheduling_reads_the_same_single_well_formed_declaration() {
        let mut headers = axum::http::HeaderMap::new();
        assert_eq!(super::super::declared_operation(&headers), None);
        headers.insert("x-quipu-operation", "bulk-export".parse().unwrap());
        assert_eq!(
            super::super::declared_operation(&headers).as_deref(),
            Some("bulk-export")
        );
        headers.append("x-quipu-operation", "bulk-export".parse().unwrap());
        assert_eq!(super::super::declared_operation(&headers), None);
        headers.remove("x-quipu-operation");
        headers.insert("x-quipu-operation", "Bulk Export".parse().unwrap());
        assert_eq!(super::super::declared_operation(&headers), None);
    }

    #[test]
    fn demo_command_attribution_before_and_after() {
        let base = quipu::request_usage::structured_request_log(
            "request_complete",
            7,
            "example-client",
            "unattributed",
            "POST",
            "/query",
            "/query",
            Some(200),
            Some(1),
            quipu::request_usage::AuthOutcome::NotRequired,
            None,
        );
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-quipu-source", "example-client.sync".parse().unwrap());
        headers.insert("x-quipu-operation", "list-items".parse().unwrap());
        let after = CommandContext::from_headers(&headers).annotate(base.clone());
        let before_value: serde_json::Value = serde_json::from_str(&base).unwrap();
        let after_value: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert!(before_value.get("declared_source").is_none());
        assert_eq!(after_value["declared_source"], "example-client.sync");
        assert_eq!(after_value["declared_operation"], "list-items");
        println!("before: {base}");
        println!("after: {after}");
    }

    #[test]
    fn failed_requests_keep_declared_labels_and_original_completion_fields() {
        // Timeout attribution must be as precise as successful reads. Labels
        // remain client declarations; neither status nor agent invents them.
        for status in [200, 408, 503] {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("x-quipu-source", "example-cli.inspect".parse().unwrap());
            headers.insert("x-quipu-operation", "list-records".parse().unwrap());
            let before = serde_json::json!({
                "event": "request_complete", "agent": "example-producer",
                "endpoint": "/query", "status": status, "elapsed_ms": 8000,
            });
            let after: serde_json::Value = serde_json::from_str(
                &CommandContext::from_headers(&headers)
                    .annotate_with(&LabelCap::new(16), before.to_string()),
            )
            .unwrap();
            assert_eq!(after["declared_source"], "example-cli.inspect");
            assert_eq!(after["declared_operation"], "list-records");
            for (key, value) in before.as_object().unwrap() {
                assert_eq!(&after[key], value);
            }
        }
        let unknown: serde_json::Value = serde_json::from_str(
            &CommandContext::from_headers(&axum::http::HeaderMap::new()).annotate_with(
                &LabelCap::new(16),
                r#"{"agent":"example-producer","status":408}"#.into(),
            ),
        )
        .unwrap();
        assert!(unknown["declared_source"].is_null());
        assert!(unknown["declared_operation"].is_null());
    }

    #[test]
    fn any_client_declares_its_own_labels_without_a_server_list() {
        // aegis-z29i / aegis-kao: Quipu holds no client vocabulary. Labels from
        // unrelated clients, including ones no release has seen, reach the log.
        for (source, operation) in [
            ("billing-worker", "nightly.rollup"),
            ("cli", "show"),
            ("a", "q"),
            ("tool-v2.reader_3", "page-7"),
        ] {
            let log = logged(Some(source), Some(operation));
            assert_eq!(log["declared_source"], source);
            assert_eq!(log["declared_operation"], operation);
            assert_eq!(log["client"], "example");
            assert_eq!(log["endpoint"], "/query");
            assert_eq!(log["status"], 200);
        }
    }

    #[test]
    fn malformed_repeated_or_reserved_declarations_stay_unknown() {
        let long = "a".repeat(quipu::request_usage::REQUEST_LABEL_MAX + 1);
        for value in [
            "Secret message",
            "1abc",
            "-abc",
            "/private/path",
            "anchor query",
            OVERFLOW,
            long.as_str(),
        ] {
            let log = logged(Some(value), Some(value));
            assert!(log["declared_source"].is_null(), "{value}");
            assert!(log["declared_operation"].is_null(), "{value}");
        }
        assert!(logged(Some(""), None)["declared_source"].is_null());
        let max = "a".repeat(quipu::request_usage::REQUEST_LABEL_MAX);
        assert_eq!(logged(Some(&max), None)["declared_source"], max.as_str());

        let mut headers = axum::http::HeaderMap::new();
        headers.append("x-quipu-source", "one".parse().unwrap());
        headers.append("x-quipu-source", "two".parse().unwrap());
        headers.insert("authorization", "Bearer hidden".parse().unwrap());
        let log: serde_json::Value =
            serde_json::from_str(&CommandContext::from_headers(&headers).annotate("{}".into()))
                .unwrap();
        assert!(log["declared_source"].is_null());
        assert!(log["declared_operation"].is_null());
        assert!(!log.to_string().contains("hidden"));
    }

    #[test]
    fn label_cardinality_is_capped_per_field_by_configuration() {
        let cap = LabelCap::new(2);
        assert_eq!(cap.admit(0, "a"), "a");
        assert_eq!(cap.admit(0, "b"), "b");
        assert_eq!(cap.admit(0, "c"), OVERFLOW);
        // Labels already admitted keep their name past the cap.
        assert_eq!(cap.admit(0, "a"), "a");
        // Fields are counted separately.
        assert_eq!(cap.admit(1, "c"), "c");
        let none = LabelCap::new(0);
        assert_eq!(none.admit(0, "a"), OVERFLOW);

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-quipu-source", "third".parse().unwrap());
        let log: serde_json::Value = serde_json::from_str(
            &CommandContext::from_headers(&headers).annotate_with(&cap, "{}".into()),
        )
        .unwrap();
        assert_eq!(log["declared_source"], OVERFLOW);
        assert!(log["declared_operation"].is_null());
    }
}
