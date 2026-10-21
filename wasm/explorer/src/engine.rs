//! General memory-store binding; the pack Explorer remains compatible.

use wasm_bindgen::prelude::*;

/// A local governed store backed by the shared Rust binding facade.
///
/// This memory constructor does not promise OPFS durability. Adopt shapes
/// explicitly, inspect capabilities, and use exact native tool names.
#[wasm_bindgen]
pub struct Database {
    engine: quipu::bindings::Engine,
}

#[wasm_bindgen]
impl Database {
    /// Create an empty memory store with native write validation enabled.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Database, JsValue> {
        Ok(Self {
            engine: quipu::bindings::Engine::open_in_memory().map_err(super::err_js)?,
        })
    }

    /// Return the actual engine build, storage and operation manifest as JSON.
    pub fn capabilities(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.engine.capabilities()).map_err(super::err_js)
    }

    /// Invoke a supported native handler with lossless JSON text in and out.
    /// Errors are JSON text including commit state and native validation data.
    pub fn invoke(&mut self, operation: &str, input: &str) -> Result<String, JsValue> {
        self.engine.invoke_json(operation, input).map_err(|error| {
            // Do not parse the result here: JS Number cannot represent every
            // native i64. Adapters choose an explicit lossless representation.
            let json = serde_json::to_string(&error).unwrap_or_else(|_| error.to_string());
            JsValue::from_str(&json)
        })
    }
}
