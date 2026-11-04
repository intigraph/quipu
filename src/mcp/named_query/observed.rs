//! Named-query observation without growing the catalog/renderer module.
use serde_json::Value as JsonValue;

/// Execute a stored or builtin named query, or list its catalog without a name.
pub fn tool_ask(store: &crate::Store, input: &JsonValue) -> crate::Result<JsonValue> {
    crate::search_outcome::observation::run(
        crate::search_outcome::Operation::NamedQuery,
        input,
        || super::ask(store, input),
    )
}

pub(super) fn json_to_param_str(v: &JsonValue) -> Option<String> {
    match v {
        JsonValue::String(s) => Some(s.clone()),
        JsonValue::Number(n) => Some(n.to_string()),
        JsonValue::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}
