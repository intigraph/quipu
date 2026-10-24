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

    /// Apply one feed frame (the SSE or WebSocket `event` and `data` text)
    /// through the shared applier. The first call makes this store a
    /// read-only replica: afterwards only its source's feed writes to it.
    /// Returns the outcome as JSON text tagged by `kind`; a protocol refusal
    /// is a returned outcome, and only a local store failure throws.
    #[wasm_bindgen(js_name = replicaApplyFrame)]
    pub fn replica_apply_frame(
        &mut self,
        event: &str,
        data: &str,
        now_ms: f64,
    ) -> Result<String, JsValue> {
        let outcome = self
            .engine
            .replica_apply_frame(event, data, millis(now_ms))
            .map_err(|error| {
                JsValue::from_str(
                    &serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()),
                )
            })?;
        serde_json::to_string(&outcome).map_err(super::err_js)
    }

    /// The replica cursor as JSON text (`{store_id, log_epoch, source_tx}`),
    /// or `null` while this store is not a replica.
    #[wasm_bindgen(js_name = replicaProgress)]
    pub fn replica_progress(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.engine.replica_progress()).map_err(super::err_js)
    }

    /// `{source_tx, last_frame_age_ms, bounded}` as JSON text. `bounded` stays
    /// false until a freshness challenge exists.
    #[wasm_bindgen(js_name = replicaFreshness)]
    pub fn replica_freshness(&self, now_ms: f64) -> Result<String, JsValue> {
        serde_json::to_string(&self.engine.replica_freshness(millis(now_ms)))
            .map_err(super::err_js)
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

/// `Date.now()` arrives as an f64; clamp it into whole non-negative milliseconds.
fn millis(now_ms: f64) -> u64 {
    if now_ms.is_finite() && now_ms > 0.0 {
        // Truncation is intended: sub-millisecond precision is meaningless here.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ms = now_ms as u64;
        ms
    } else {
        0
    }
}
