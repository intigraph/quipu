//! Bounded private evidence. Redaction is best effort, never anonymization.
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::sync::OnceLock;

/// Request/response context for private journals, never metric labels or logs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Details {
    /// Query with recognized credential forms redacted; absent when oversized.
    pub query_evidence: Option<String>,
    /// Whether redaction changed text. Unknown means text was not retained.
    pub query_redacted: Option<bool>,
    /// Original query byte length, distinct from retained evidence length.
    pub query_bytes: Option<usize>,
    /// Bounded retrieval options; arbitrary params and embeddings are omitted.
    pub requested_options: Option<Value>,
    /// Resolved graph coverage reported by the actual search response.
    pub resolved_scope: Option<Value>,
    /// Admission reason from a validated contract, retained only when known.
    pub context_reason: Option<String>,
    /// Build-script declaration; not proof of deployed executable identity.
    pub reported_build_revision: Option<&'static str>,
    /// No revision is guessed when the index does not expose one.
    pub index_revision: Option<String>,
    /// No revision is guessed when the configuration does not expose one.
    pub config_revision: Option<String>,
}

pub(super) fn scrub(text: &str) -> String {
    static SECRET: OnceLock<Regex> = OnceLock::new();
    static OPAQUE: OnceLock<Regex> = OnceLock::new();
    let secret = SECRET.get_or_init(|| Regex::new(r#"(?i)\b(?:bearer\s+|(?:api[_-]?key|token|password|secret)\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s,;]+)"#).unwrap());
    let opaque = OPAQUE.get_or_init(|| Regex::new(r"(?i)\b(?:sk-[a-z0-9_-]+|gh[pousr]_[a-z0-9_]+|github_pat_[a-z0-9_]+|[a-z0-9_+/=-]{32,})\b").unwrap());
    opaque
        .replace_all(&secret.replace_all(text, "[REDACTED]"), "[REDACTED]")
        .into_owned()
}

pub(super) fn scrub_declaration(key: &str, text: &str) -> String {
    // A UUID session identity is not an opaque credential. This exception is
    // restricted to the declared session field; query text keeps full scrubbing.
    let uuid = text.len() == 36
        && text.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        });
    if key == "session" && uuid {
        text.into()
    } else {
        scrub(text)
    }
}

pub(super) fn within_limits(value: &Value) -> bool {
    let mut pending = vec![(value, 0)];
    let mut visited = 0;
    let mut bytes = 0;
    while let Some((value, depth)) = pending.pop() {
        visited += 1;
        if visited > 4096 || depth > 16 {
            return false;
        }
        match value {
            Value::String(text) => bytes += text.len(),
            Value::Array(rows) => {
                if visited + pending.len() + rows.len() > 4096 {
                    return false;
                }
                pending.extend(rows.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(fields) => {
                if visited + pending.len() + fields.len() > 4096 {
                    return false;
                }
                for (key, value) in fields {
                    bytes += key.len();
                    pending.push((value, depth + 1));
                }
            }
            _ => (),
        }
        if bytes > 65536 {
            return false;
        }
    }
    true
}

fn sanitized(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(scrub(text)),
        Value::Array(rows) => Value::Array(rows.iter().map(sanitized).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    let secret = ["authorization", "password", "secret", "token", "api_key"]
                        .contains(&key.to_ascii_lowercase().as_str());
                    (
                        key.clone(),
                        if secret {
                            Value::String("[REDACTED]".into())
                        } else {
                            sanitized(value)
                        },
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn bounded(value: Value) -> Option<Value> {
    (value.to_string().len() <= 4096).then_some(value)
}

impl Details {
    pub(super) fn capture(
        input: &Value,
        response: Option<&Value>,
        validated_context: bool,
    ) -> Self {
        let query = input.get("query").and_then(Value::as_str);
        let retained = query.filter(|text| text.len() <= 4096).map(scrub);
        let options: serde_json::Map<String, Value> = [
            "graph",
            "graphs",
            "all_graphs",
            "ranking",
            "mode",
            "limit",
            "valid_at",
            "entity_type",
            "group_ids",
            "filters",
            "anchor",
            "anchor_mode",
            "max_hops",
            "fusion",
        ]
        .into_iter()
        .filter_map(|key| {
            input
                .get(key)
                .filter(|value| within_limits(value))
                .map(|value| (key.into(), sanitized(value)))
        })
        .collect();
        Self {
            query_redacted: retained
                .as_deref()
                .zip(query)
                .map(|(kept, original)| kept != original),
            query_evidence: retained,
            query_bytes: query.map(str::len),
            requested_options: within_limits(input)
                .then(|| bounded(Value::Object(options)))
                .flatten(),
            resolved_scope: response
                .and_then(|value| value.get("search_scope"))
                .filter(|value| within_limits(value))
                .and_then(|value| bounded(sanitized(value))),
            context_reason: validated_context
                .then(|| {
                    response?
                        .get("context")?
                        .get("reason")?
                        .as_str()
                        .filter(|text| text.len() <= 128)
                        .map(String::from)
                })
                .flatten(),
            reported_build_revision: option_env!("QUIPU_GIT_SHA"),
            index_revision: None,
            config_revision: None,
        }
    }
}

#[cfg(test)]
mod tests;
