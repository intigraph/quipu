//! Browser bridge to the same portable overlay/envelope used by native clients.
//! No key registration, master admission, HTTP write or private-key JSON.

use quipu::client_changeset::{BaseJournal, ClientOverlay, ProposalIdentity, SignedChangeSet};
use wasm_bindgen::prelude::*;

use super::err_js;

/// Open private proposals over an acquired source journal. The master still
/// reconstructs the base and checks current authority at admission.
#[wasm_bindgen(js_name = openClientOverlay)]
pub fn open_client_overlay(journal_json: &str, fork_id: &str) -> Result<String, JsValue> {
    let journal: BaseJournal = serde_json::from_str(journal_json).map_err(err_js)?;
    let overlay = ClientOverlay::open(journal, fork_id.to_owned()).map_err(err_js)?;
    serde_json::to_string(&overlay).map_err(err_js)
}

/// Replace only local proposal triples; the frozen journal is retained.
#[wasm_bindgen(js_name = replaceClientOverlay)]
pub fn replace_client_overlay(overlay_json: &str, triples_json: &str) -> Result<String, JsValue> {
    let mut overlay: ClientOverlay = serde_json::from_str(overlay_json).map_err(err_js)?;
    let triples: Vec<String> = serde_json::from_str(triples_json).map_err(err_js)?;
    overlay.replace(triples).map_err(err_js)?;
    serde_json::to_string(&overlay).map_err(err_js)
}

/// Prepare canonical bytes for a caller-held key, for example a non-exportable
/// WebCrypto key. The browser never reimplements the envelope serialization.
#[wasm_bindgen(js_name = prepareClientChangeset)]
pub fn prepare_client_changeset(
    overlay_json: &str,
    identity_json: &str,
) -> Result<String, JsValue> {
    let overlay: ClientOverlay = serde_json::from_str(overlay_json).map_err(err_js)?;
    let identity: ProposalIdentity = serde_json::from_str(identity_json).map_err(err_js)?;
    let prepared = overlay.prepare(identity).map_err(err_js)?;
    let message = String::from_utf8(prepared.message().map_err(err_js)?).map_err(err_js)?;
    Ok(
        serde_json::json!({"envelope":prepared.envelope,"body":prepared.body,"message":message})
            .to_string(),
    )
}

/// Verify content and signature with the shared crate. This is not a trust
/// decision: only the master may check current registration/scope and admit.
#[wasm_bindgen(js_name = verifyClientChangesetSignature)]
pub fn verify_client_changeset_signature(
    proposal_json: &str,
    public_key: &str,
) -> Result<bool, JsValue> {
    let proposal: SignedChangeSet = serde_json::from_str(proposal_json).map_err(err_js)?;
    proposal.verify(public_key).map_err(err_js)?;
    Ok(true)
}
