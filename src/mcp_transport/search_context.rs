//! MCP emits admitted context only; REST retains diagnostic ranked candidates.
use serde_json::{Value, json};

pub(super) fn render(text: &str) -> Result<String, &'static str> {
    let response: Value =
        serde_json::from_str(text).map_err(|_| "UNKNOWN: invalid search context response")?;
    let context = &response["context"];
    if context["contract"].as_str() != Some("quipu-context/v1") || response.get("error").is_some() {
        return Err("UNKNOWN: search context contract unavailable");
    }
    let floor = context["similarity_floor"]
        .as_f64()
        .filter(|f| f.is_finite() && (0.52..=1.0).contains(f))
        .ok_or("UNKNOWN: invalid context floor")?;
    let rows = match (
        response["insufficient_context"].as_bool(),
        context["outcome"].as_str(),
    ) {
        (Some(true), Some("NO CONTEXT")) => Vec::new(),
        (Some(false), Some("CONTEXT")) => {
            let rows = response["context_results"]
                .as_array()
                .filter(|rows| !rows.is_empty())
                .ok_or("UNKNOWN: missing admitted context")?;
            if !rows.iter().all(|row| {
                row["similarity"]
                    .as_f64()
                    .is_some_and(|s| s.is_finite() && (floor..=1.0).contains(&s))
            }) {
                return Err("UNKNOWN: invalid admitted similarity");
            }
            rows.clone()
        }
        _ => return Err("UNKNOWN: invalid context outcome"),
    };
    let mut view = json!({"context":context,"insufficient_context":response["insufficient_context"],
        "results":rows,"count":rows.len(),"context_results":rows});
    for key in ["scoped", "ranking"] {
        if let Some(value) = response.get(key) {
            view[key] = value.clone();
        }
    }
    Ok(view.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_outcome_requires_supported_floor() {
        for floor in [
            json!(0.2),
            Value::Null,
            json!(true),
            json!(1.1),
            json!(0.52),
        ] {
            let body = json!({"insufficient_context":true,"context_results":[],
                "context":{"contract":"quipu-context/v1","outcome":"NO CONTEXT","similarity_floor":floor}});
            assert_eq!(render(&body.to_string()).is_ok(), floor == json!(0.52));
        }
    }
}
