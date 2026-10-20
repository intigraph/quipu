//! Actual namespace handler and auth classifications; no live service involved.
use axum::extract::State;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn namespace_list_and_authenticated_mint_surfaces_use_the_configured_registry() {
    let mut store = quipu::Store::open_in_memory().unwrap();
    let mut config = quipu::QuipuConfig::default();
    config.namespaces.push(quipu::entity_namespaces::Namespace {
        prefix: "p".into(),
        ..Default::default()
    });
    quipu::entity_namespaces::configure(&mut store, &config).unwrap();
    let shared: super::SharedStore = Arc::new(super::StoreHandle::writer_only(store));
    let response = super::tools::namespaces_get(State(shared.clone()))
        .await
        .unwrap()
        .0;
    assert_eq!(response["namespaces"][0]["prefix"], "p");
    let response = super::tools::namespaces(State(shared.clone()), axum::Json(json!({})))
        .await
        .unwrap()
        .0;
    assert_eq!(response["namespaces"][0]["audience"], "public");
    assert!(!quipu::http_auth::is_write_request("/namespaces", "POST"));
    assert!(quipu::http_auth::is_write_request("/entity_ref", "POST"));
    quipu::entity_namespaces::apply(&mut shared.lock()).unwrap();
    let minted = super::tools::entity_ref(
        State(shared),
        axum::Json(json!({
            "action":"mint","namespace":"p","type":"urn:example:Host"
        })),
    )
    .await
    .unwrap()
    .0;
    assert!(
        minted["iri"]
            .as_str()
            .unwrap()
            .starts_with("https://w3id.org/intigraph/")
    );
}
