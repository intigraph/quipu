//! Validate the bounded schema subset used by portable workflow tools.
use rmcp::ErrorData;
use serde_json::Value;

pub(super) fn validate(schema: &Value, value: &Value, path: &str) -> Result<(), ErrorData> {
    let invalid = || ErrorData::invalid_params(format!("invalid tool field {path}"), None);
    let matches = |kind: &str| match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    };
    if let Some(kind) = schema.get("type") {
        let valid = kind.as_str().map_or_else(
            || {
                kind.as_array()
                    .is_some_and(|kinds| kinds.iter().any(|k| k.as_str().is_some_and(matches)))
            },
            matches,
        );
        if !valid {
            return Err(invalid());
        }
    }
    if schema
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|choices| !choices.contains(value))
    {
        return Err(invalid());
    }
    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64)
        && value.as_f64().is_some_and(|n| n < minimum)
    {
        return Err(invalid());
    }
    if let Some(fields) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !fields.contains_key(key) {
                    return Err(ErrorData::invalid_params(
                        format!("missing tool field {path}.{key}"),
                        None,
                    ));
                }
            }
        }
        for (key, value) in fields {
            if let Some(field_schema) = schema.get("properties").and_then(|p| p.get(key)) {
                validate(field_schema, value, &format!("{path}.{key}"))?;
            } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                return Err(ErrorData::invalid_params(
                    format!("unknown tool field {path}.{key}"),
                    None,
                ));
            }
        }
    }
    if let Some(items) = schema.get("items")
        && let Some(values) = value.as_array()
    {
        for value in values {
            validate(items, value, path)?;
        }
    }
    Ok(())
}
