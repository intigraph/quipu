//! Additive store operations over the same native Rust handlers.

use wasm_bindgen::prelude::*;

use super::{Explorer, err_js, features};

#[wasm_bindgen]
impl Explorer {
    /// Assert RDF through the native knot handler, including vocabulary,
    /// SHACL, governance, named-graph and temporal admission. A result with
    /// `conforms: false` is a refusal, not a successful write.
    pub fn knot(&mut self, input: &str) -> Result<String, JsValue> {
        let input = serde_json::from_str(input).map_err(err_js)?;
        let result = quipu::tool_knot(&mut self.store, &input).map_err(features::engine_error)?;
        if result
            .get("tx_id")
            .and_then(serde_json::Value::as_i64)
            .is_some()
        {
            self.record("knot", &result);
        }
        serde_json::to_string(&result).map_err(err_js)
    }

    /// Explicitly manage the shape catalogue through the native tool contract.
    /// Merely listing or reading shapes does not add an edit-log entry.
    pub fn shapes(&mut self, input: &str) -> Result<String, JsValue> {
        let input: serde_json::Value = serde_json::from_str(input).map_err(err_js)?;
        let result = quipu::tool_shapes(&self.store, &input).map_err(features::engine_error)?;
        if matches!(
            input.get("action").and_then(serde_json::Value::as_str),
            Some("load" | "remove")
        ) && result.get("found") != Some(&serde_json::Value::Bool(false))
        {
            self.record("shapes", &result);
        }
        serde_json::to_string(&result).map_err(err_js)
    }

    /// Native entity history for ROOT, including retracted facts. This does
    /// not claim composed or named-graph audit completeness.
    #[wasm_bindgen(js_name = entityHistory)]
    pub fn entity_history(&self, iri: &str) -> Result<String, JsValue> {
        let input = serde_json::json!({ "iri": iri });
        let result =
            quipu::tool_entity_history(&self.store, &input).map_err(features::engine_error)?;
        serde_json::to_string(&result).map_err(err_js)
    }
}
