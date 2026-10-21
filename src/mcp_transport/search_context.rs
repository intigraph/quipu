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
        (Some(true), Some("NO CONTEXT")) => {
            let rows = response["context_results"]
                .as_array()
                .filter(|rows| rows.is_empty())
                .ok_or("UNKNOWN: invalid negative context evidence")?;
            rows.clone()
        }
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
    for key in ["scoped", "ranking", "search_scope"] {
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
    fn coverage_survives_context_projection_even_for_an_empty_result() {
        let body = json!({"insufficient_context":true,"context_results":[],
            "results":[{"entity":"urn:diagnostic-only"}],
            "search_scope":{"graphs":["urn:example:project"],"selection":"default"},
            "context":{"contract":"quipu-context/v1","outcome":"NO CONTEXT","similarity_floor":0.52}});
        let view: Value = serde_json::from_str(&render(&body.to_string()).unwrap()).unwrap();
        assert_eq!(view["search_scope"], body["search_scope"]);
        assert_eq!(view["results"], json!([]));
    }

    #[test]
    fn negative_outcome_requires_explicit_empty_context_rows() {
        for rows in [
            json!([{"entity":"urn:wrong","similarity":0.8}]),
            Value::Null,
            json!({}),
        ] {
            let body = json!({"insufficient_context":true,"context_results":rows,
                "context":{"contract":"quipu-context/v1","outcome":"NO CONTEXT","similarity_floor":0.52}});
            assert!(render(&body.to_string()).is_err());
        }
        let missing = json!({"insufficient_context":true,
            "context":{"contract":"quipu-context/v1","outcome":"NO CONTEXT","similarity_floor":0.52}});
        assert!(render(&missing.to_string()).is_err());
    }

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
