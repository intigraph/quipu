//! Native receiving phases, observed without changing their decisions.
use super::{Explorer, err_js, features};
use quipu::share_import::{PromoteImportRequest, promote_import};
use quipu::share_transport::read_archive_bytes;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn now() -> f64;
}

struct Progress {
    callback: Option<js_sys::Function>,
    started: f64,
    previous: f64,
    phase: &'static str,
    timings: Vec<serde_json::Value>,
}
impl Progress {
    fn new(callback: Option<js_sys::Function>) -> Self {
        let started = now();
        let result = Self {
            callback,
            started,
            previous: started,
            phase: "unpack",
            timings: Vec::new(),
        };
        result.emit("unpack", 0.0);
        result
    }
    fn emit(&self, phase: &str, elapsed: f64) {
        if let Some(callback) = &self.callback {
            let value = serde_json::json!({"phase": phase, "elapsed_ms": elapsed});
            // Progress is observational: a UI failure cannot change admission.
            let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(&value.to_string()));
        }
    }
    fn step(&mut self, next: &'static str) {
        let current = now();
        self.timings
            .push(serde_json::json!({"phase":self.phase,"duration_ms":current-self.previous}));
        self.emit(next, current - self.started);
        self.previous = current;
        self.phase = next;
    }
    fn finish(mut self) -> Vec<serde_json::Value> {
        self.step("complete");
        self.timings
    }
}

pub(super) fn load(
    bytes: &[u8],
    source: &str,
    timestamp: &str,
    callback: Option<js_sys::Function>,
) -> Result<Explorer, JsValue> {
    let mut progress = Progress::new(callback);
    let mut request = read_archive_bytes(bytes, source, true).map_err(err_js)?;
    request.actor = None;
    progress.step("open_store");
    let mut store = quipu::Store::open_in_memory().map_err(err_js)?;
    #[cfg(feature = "shacl")]
    {
        // This explorer writes, so a linked validator must also enforce
        // adopted shapes on writes rather than merely preview imports.
        store.shacl_config_mut().validate_on_write = true;
    }

    progress.step("adopt_shapes");
    // Step 2. Adopting the pack's shapes is a DECISION the receiver makes,
    // and skipping it is not a silent no-op: `import_share` would find the
    // pack's classes ungoverned and quarantine every triple. The page shows
    // this as its own line for that reason.
    store
        .load_shapes("repository-share", &request.shapes_turtle, timestamp)
        .map_err(err_js)?;
    features::register_reasoners(&mut store)?;

    progress.step("verify_import");
    let import =
        quipu::share_import::import_share(&mut store, &request, timestamp, None).map_err(err_js)?;

    progress.step("promote");
    let promotion = if import.promotion.eligible {
        Some(
            promote_import(
                &mut store,
                &PromoteImportRequest {
                    share_id: import.share_id.clone(),
                    actor: None,
                    target_graph: None,
                    retire_from: None,
                    dry_run: false,
                },
                timestamp,
                None,
            )
            .map_err(err_js)?,
        )
    } else {
        None
    };

    let timings = progress.finish();
    let load_report = serde_json::to_string(&serde_json::json!({
        "manifest": request.manifest,
        "load_timings": timings,
        "import": import,
        "promotion": promotion,
        "shapes_bytes": request.shapes_turtle.len(),
        "export_bytes": request.export_ntriples.len(),
        // Named rather than inferred: callers can distinguish a validator
        // result from the fallback used by builds without SHACL.
        "shacl_compiled": cfg!(feature = "shacl"),
    }))
    .map_err(err_js)?;

    Ok(Explorer {
        store,
        load_report,
        parent_share: request.manifest.share_id.clone(),
        parent_graph_hash: request.manifest.graph_hash.clone(),
        parent_export_ntriples: request.export_ntriples.clone(),
        edits: Vec::new(),
    })
}
