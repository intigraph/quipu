//! Native handler boundary; scoped-principal denials are exercised separately.
use super::*;
use axum::{extract::State, http::HeaderMap};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn mapping_handlers_prepare_apply_refuse_stale_and_retire() {
    let mut native = quipu::Store::open_in_memory().unwrap();
    for graph in [
        "urn:fixture:from",
        "urn:fixture:to",
        "urn:fixture:mappings",
        "urn:quipu:graph:operations",
    ] {
        native.graph_create(graph).unwrap();
    }
    native
        .load_shapes(
            "fixture",
            "<urn:fixture:s> <urn:fixture:p> <urn:fixture:o> .",
            "2026-10-25T00:00:00Z",
        )
        .unwrap();
    let pins = native
        .current_shape_pins(quipu::shape_mapping::RegistryKind::Shapes, "fixture")
        .unwrap();
    let before = native.latest_tx_id().unwrap();
    let shared = Arc::new(super::super::StoreHandle::writer_only(native));
    let mapping = json!({"id":"urn:fixture:mapping","graph":"urn:fixture:mappings",
        "maps_from":["urn:fixture:from"],"maps_to":["urn:fixture:to"],"pins":pins,"rows":[{
            "subject_id":"urn:fixture:Ticket","predicate_id":"http://www.w3.org/2002/07/owl#equivalentClass",
            "object_id":"urn:fixture:Task","mapping_justification":"semapv:ManualMappingCuration",
            "author_id":"urn:fixture:reviewer","quipu_review":"accepted"}]});
    let Json(proposal) = super::super::tools::mapping_propose(
        State(shared.clone()),
        Json(json!({"mapping":mapping})),
    )
    .await
    .unwrap();
    let Json(decided) = decide(Json(json!({"proposal":proposal,"reviewer":"urn:fixture:reviewer","decisions":[{
        "subject_id":"urn:fixture:Ticket","predicate_id":"http://www.w3.org/2002/07/owl#equivalentClass",
        "object_id":"urn:fixture:Task","accept":true}]}))).await.unwrap();
    assert_eq!(shared.lock().latest_tx_id().unwrap(), before);
    let input = json!({"action":"apply","mapping":decided["mapping"],"expected_digest":decided["expected_digest"],"actor":"urn:fixture:reviewer"});
    let Json(applied) = super::super::tools::mapping_apply(
        State(shared.clone()),
        HeaderMap::new(),
        None,
        Json(input.clone()),
    )
    .await
    .unwrap();
    assert_eq!(applied["status"], "decided");
    let settled = shared.lock().latest_tx_id().unwrap();
    assert_eq!(applied["tx"], settled);
    let mut stale = input.clone();
    stale["expected_digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    assert!(
        super::super::tools::mapping_apply(
            State(shared.clone()),
            HeaderMap::new(),
            None,
            Json(stale)
        )
        .await
        .is_err()
    );
    assert_eq!(shared.lock().latest_tx_id().unwrap(), settled);
    let Json(retired) = super::super::tools::mapping_apply(State(shared.clone()), HeaderMap::new(), None,
        Json(json!({"action":"retire","mapping":"urn:fixture:mapping","expected_digest":decided["expected_digest"],"actor":"urn:fixture:reviewer"}))).await.unwrap();
    assert_eq!(retired["status"], "retired");
    assert_eq!(retired["assert_writable"]["urn:fixture:Task"], false);
}
