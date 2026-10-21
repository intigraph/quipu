//! Receiver-driven first-use pointers. Transport and trust stay with the host.
use std::collections::BTreeMap;

use quipu::share_pointer_load::LazyPacks;
use wasm_bindgen::prelude::*;

use crate::err_js;

/// A read-only pointer consumer with a separate store and explicit local shapes
/// authority. Loading declarations never downloads or composes pointed packs.
#[wasm_bindgen]
pub struct PointerExplorer {
    loader: LazyPacks,
    store: quipu::Store,
    cache: BTreeMap<String, Vec<u8>>,
}

#[wasm_bindgen]
impl PointerExplorer {
    /// Register archives. `authority` must be selected by the receiving page;
    /// it must not come from the downloaded pointer target.
    #[wasm_bindgen(constructor)]
    pub fn new(declaration: &[u8], authority: &[u8]) -> Result<PointerExplorer, JsValue> {
        let read = |bytes: &[u8], name| {
            quipu::share_transport::read_archive_bytes(
                bytes,
                name,
                bytes.starts_with(&[0x1f, 0x8b]),
            )
            .map_err(err_js)
        };
        Ok(Self {
            loader: LazyPacks::new(
                read(declaration, "declaration")?,
                read(authority, "local-authority")?,
            )
            .map_err(err_js)?,
            store: quipu::Store::open_in_memory().map_err(err_js)?,
            cache: BTreeMap::new(),
        })
    }

    /// A declared pointer as JSON, without fetching it.
    pub fn pointer(&self, graph: &str) -> Result<String, JsValue> {
        serde_json::to_string(self.loader.pointer(graph).map_err(err_js)?).map_err(err_js)
    }

    /// Whether this instance already retained an archive. Trust must still be
    /// rechecked by useGraph, and cached bytes are reverified there.
    #[wasm_bindgen(js_name = hasCached)]
    pub fn has_cached(&self, graph: &str) -> Result<bool, JsValue> {
        let pointer = self.loader.pointer(graph).map_err(err_js)?;
        let quipu::share::PointerTarget::Pack { sha256, .. } = &pointer.target else {
            unreachable!()
        };
        Ok(self.cache.contains_key(&format!(
            "sha256:{}",
            sha256.strip_prefix("sha256:").unwrap_or(sha256)
        )))
    }

    /// Verify supplied bytes (or reuse verified cached bytes) and compose on
    /// graph use. The host policy callback receives the declaring share ID and
    /// pointer. Only the literal boolean true authorizes loading; exceptions,
    /// promises and missing decisions refuse. No foreign shapes enter policy.
    #[wasm_bindgen(js_name = useGraph)]
    pub fn use_graph(
        &mut self,
        graph: &str,
        bytes: &[u8],
        trust: &js_sys::Function,
        timestamp: &str,
    ) -> Result<String, JsValue> {
        let result = self.loader.use_graph(&mut self.store, graph, &mut self.cache,
            |declaration, pointer| {
                let input = serde_json::json!({ "declaring_share": declaration.manifest.share_id, "pointer": pointer });
                let decision = trust.call1(&JsValue::NULL, &JsValue::from_str(&input.to_string()));
                if !matches!(decision, Ok(value) if value.as_bool() == Some(true)) {
                    return Err(quipu::Error::InvalidValue("receiver pointer trust refused".into()));
                }
                Ok(())
            }, |_| Ok(bytes.to_vec()), timestamp, None).map_err(err_js)?;
        serde_json::to_string(&result).map_err(err_js)
    }

    /// Query an explicit composed dataset (the JSON from useGraph names it).
    pub fn query(&self, sparql: &str) -> Result<String, JsValue> {
        let out = quipu::tool_query(&self.store, &serde_json::json!({ "query": sparql }))
            .map_err(err_js)?;
        serde_json::to_string(&out).map_err(err_js)
    }
}
