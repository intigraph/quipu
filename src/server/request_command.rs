//! Finite diagnostic caller declarations; never authorization or metric labels.

#[derive(Clone, Default)]
pub(super) struct CommandContext {
    source: Option<String>,
    operation: Option<String>,
}

impl CommandContext {
    pub(super) fn from_headers(headers: &axum::http::HeaderMap) -> Self {
        let one = |name: &str| {
            let mut values = headers.get_all(name).iter();
            let value = values.next()?.to_str().ok()?;
            values.next().is_none().then_some(value)
        };
        let source = one("x-quipu-source").filter(|value| {
            matches!(*value, "seeds" | "shantytown")
                || ["st-", "sd-"].iter().any(|prefix| {
                    value.strip_prefix(prefix).is_some_and(|command| {
                        matches!(
                            command,
                            "anchor"
                                | "plate"
                                | "inbox"
                                | "stop_policy"
                                | "stop_event"
                                | "haul"
                                | "tend"
                                | "work"
                                | "other"
                                | "show"
                                | "list"
                                | "ready"
                                | "where"
                                | "search"
                                | "count"
                                | "info"
                                | "stats"
                                | "create"
                                | "update"
                                | "close"
                                | "comments"
                        )
                    })
                })
        });
        let operation = one("x-quipu-operation").filter(|value| {
            matches!(
                *value,
                "query"
                    | "health"
                    | "graphs"
                    | "show"
                    | "list"
                    | "ready"
                    | "where"
                    | "search"
                    | "count"
                    | "info"
                    | "stats"
                    | "tend"
                    | "ready-count"
                    | "uses-native-schema"
                    | "closed-status"
                    | "closed-statuses"
                    | "owner-shape-supported"
                    | "owner-rows"
                    | "deferred-rows"
                    | "unassigned-counts"
                    | "unblocked-ids"
                    | "due-rows"
                    | "decision-lists"
                    | "create"
                    | "update"
                    | "close"
                    | "comments"
            )
        });
        Self {
            source: source.map(str::to_owned),
            operation: operation.map(str::to_owned),
        }
    }

    pub(super) fn annotate(&self, log: String) -> String {
        let Ok(serde_json::Value::Object(mut record)) = serde_json::from_str(&log) else {
            return log;
        };
        // Null is UNKNOWN. Do not guess a command from the endpoint or agent.
        record.insert("declared_source".into(), serde_json::json!(self.source));
        record.insert(
            "declared_operation".into(),
            serde_json::json!(self.operation),
        );
        serde_json::Value::Object(record).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_command_attribution_before_and_after() {
        let base = quipu::request_usage::structured_request_log(
            "request_complete",
            7,
            "seeds",
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
        headers.insert("x-quipu-source", "st-anchor".parse().unwrap());
        headers.insert("x-quipu-operation", "list".parse().unwrap());
        let after = CommandContext::from_headers(&headers).annotate(base.clone());
        let before_value: serde_json::Value = serde_json::from_str(&base).unwrap();
        let after_value: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert!(before_value.get("declared_source").is_none());
        assert_eq!(after_value["declared_source"], "st-anchor");
        assert_eq!(after_value["declared_operation"], "list");
        println!("before: {base}");
        println!("after: {after}");
    }

    #[test]
    fn only_known_single_values_reach_the_log() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-quipu-source", "st-anchor".parse().unwrap());
        headers.insert("x-quipu-operation", "list".parse().unwrap());
        headers.insert("authorization", "Bearer hidden".parse().unwrap());
        let context = CommandContext::from_headers(&headers);
        let log: serde_json::Value = serde_json::from_str(&context.annotate("{}".into())).unwrap();
        assert_eq!(log["declared_source"], "st-anchor");
        assert_eq!(log["declared_operation"], "list");
        assert!(!log.to_string().contains("hidden"));
        headers.append("x-quipu-source", "st-inbox".parse().unwrap());
        headers.insert("x-quipu-operation", "private-argument".parse().unwrap());
        let log: serde_json::Value =
            serde_json::from_str(&CommandContext::from_headers(&headers).annotate("{}".into()))
                .unwrap();
        assert!(log["declared_source"].is_null());
        assert!(log["declared_operation"].is_null());
    }

    #[test]
    fn unknown_or_missing_declarations_are_explicit_without_inference() {
        for source in [
            None,
            Some("st-secret-message"),
            Some("/private/path"),
            Some("st-anchor query"),
        ] {
            let mut headers = axum::http::HeaderMap::new();
            if let Some(source) = source {
                headers.insert("x-quipu-source", source.parse().unwrap());
            }
            let context = CommandContext::from_headers(&headers);
            let log: serde_json::Value = serde_json::from_str(
                &context.annotate("{\"client\":\"seeds\",\"endpoint\":\"/query\"}".into()),
            )
            .unwrap();
            assert!(log["declared_source"].is_null());
            assert!(log["declared_operation"].is_null());
        }
    }

    #[test]
    fn native_projection_operations_are_retained() {
        // Finite operation names already emitted by Shantytown's native reads.
        // They are diagnostic declarations, not an authenticated caller identity.
        for operation in [
            "uses-native-schema",
            "closed-status",
            "closed-statuses",
            "owner-shape-supported",
            "owner-rows",
            "deferred-rows",
            "unassigned-counts",
            "unblocked-ids",
            "due-rows",
            "decision-lists",
        ] {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("x-quipu-source", "st-anchor".parse().unwrap());
            headers.insert("x-quipu-operation", operation.parse().unwrap());
            let log: serde_json::Value =
                serde_json::from_str(&CommandContext::from_headers(&headers).annotate(
                    "{\"client\":\"seeds\",\"endpoint\":\"/query\",\"status\":200}".into(),
                ))
                .unwrap();
            assert_eq!(log["declared_operation"], operation, "{operation}");
            assert_eq!(log["declared_source"], "st-anchor");
            assert_eq!(log["client"], "seeds");
            assert_eq!(log["endpoint"], "/query");
            assert_eq!(log["status"], 200);
        }
    }

    #[test]
    fn unknown_and_repeated_native_operations_remain_unknown() {
        for operation in ["private-operation", "owner-rows private", "/private/path"] {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("x-quipu-operation", operation.parse().unwrap());
            let log: serde_json::Value =
                serde_json::from_str(&CommandContext::from_headers(&headers).annotate("{}".into()))
                    .unwrap();
            assert!(log["declared_operation"].is_null());
        }
        let mut headers = axum::http::HeaderMap::new();
        headers.append("x-quipu-operation", "owner-rows".parse().unwrap());
        headers.append("x-quipu-operation", "due-rows".parse().unwrap());
        let log: serde_json::Value =
            serde_json::from_str(&CommandContext::from_headers(&headers).annotate("{}".into()))
                .unwrap();
        assert!(log["declared_operation"].is_null());
    }
}
