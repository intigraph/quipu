//! Trusted, same-release default-store snapshots. Arbitrary packs use loading.rs.
use super::{Explorer, err_js, features};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl Explorer {
    /// Serialize an unedited store after full native import and promotion.
    ///
    /// # Errors
    /// Refuses edits, blocking findings, or an incomplete receiving decision.
    #[wasm_bindgen(js_name = verifiedSnapshot)]
    pub fn verified_snapshot(&self) -> Result<Vec<u8>, JsValue> {
        if !self.edits.is_empty() {
            return Err(JsValue::from_str(
                "edited stores are not default-pack snapshots",
            ));
        }
        check_report(&self.load_report)?;
        self.store.serialize_db().map_err(err_js)
    }

    /// Restore a producer-validated default store, not an arbitrary imported DB.
    ///
    /// The host MUST first authenticate snapshot, pack and report against its
    /// same-release pinned proof. This is deliberately a separate API from
    /// loadPendant: a self-asserted report is not validation or authentication.
    /// Writes still enforce the adopted shapes and the normal native write gate.
    ///
    /// # Errors
    /// Refuses a missing receiving decision, pack/report mismatch, or invalid DB.
    #[wasm_bindgen(js_name = restoreVerifiedSnapshot)]
    pub fn restore_verified_snapshot(
        snapshot: &[u8],
        pack: &[u8],
        source: &str,
        report: &str,
    ) -> Result<Explorer, JsValue> {
        let decision = check_report(report)?;
        let request =
            quipu::share_transport::read_archive_bytes(pack, source, true).map_err(err_js)?;
        if serde_json::to_value(&request.manifest).map_err(err_js)? != decision["manifest"] {
            return Err(JsValue::from_str("snapshot report belongs to another pack"));
        }
        let mut store = quipu::Store::open_from_bytes(snapshot).map_err(err_js)?;
        #[cfg(feature = "shacl")]
        {
            store.shacl_config_mut().validate_on_write = true;
        }
        features::register_reasoners(&mut store)?;
        Ok(Explorer {
            store,
            load_report: report.to_owned(),
            parent_share: request.manifest.share_id.clone(),
            parent_graph_hash: request.manifest.graph_hash.clone(),
            parent_export_ntriples: request.export_ntriples,
            edits: Vec::new(),
        })
    }
}

fn check_report(report: &str) -> Result<serde_json::Value, JsValue> {
    let value: serde_json::Value = serde_json::from_str(report).map_err(err_js)?;
    let manifest = &value["manifest"];
    if value["import"]["share_id"] != manifest["share_id"]
        || value["import"]["graph_hash"] != manifest["graph_hash"]
        || value["promotion"]["share_id"] != manifest["share_id"]
        || !value["promotion"]["target_graph"].is_null()
        || value["shacl_compiled"] != true
        || !value["import"]["validation"]["conforms"].is_boolean()
        || value["import"]["promotion"]["eligible"] != true
        || value["import"]["validation"]["off_vocabulary"]
            .as_array()
            .is_none_or(|items| !items.is_empty())
        || value["import"]["validation"]["blocking"] != false
        || value["promotion"]["outcome"] != "promoted"
    {
        return Err(JsValue::from_str(
            "default snapshot requires completed native validation and eligible promotion",
        ));
    }
    Ok(value)
}
