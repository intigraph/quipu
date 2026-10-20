//! Actual import handler acceptance with a real configured ONNX provider.
use axum::extract::State;
use quipu::KnowledgeVectorStore;
use quipu::{
    Store,
    share::{SHARE_PAYLOAD_MAX_BYTES, ShareOptions, ShareScope},
};
use std::{path::PathBuf, sync::Arc};

#[cfg(feature = "onnx")]
#[tokio::test]
#[ignore = "requires QUIPU_TEST_MODEL and QUIPU_TEST_TOKENIZER for real native inference"]
async fn actual_onnx_http_import_reports_indexing_and_preserves_staging() {
    let ts = "2026-10-20T00:00:00Z";
    let mut producer = Store::open_in_memory().unwrap();
    let catalogue = producer.overlay_create("urn:test:catalogue", 0).unwrap();
    quipu::rdf::ingest_rdf_to_graph(
        &mut producer,
        include_bytes!("../../tests/fixtures/share-catalogue.ttl").as_slice(),
        oxrdfio::RdfFormat::Turtle,
        None,
        ts,
        None,
        None,
        catalogue,
    )
    .unwrap();
    let graph = producer.graph_create("urn:fixture:repository").unwrap();
    quipu::rdf::ingest_rdf_to_graph(&mut producer,
        b"<urn:fixture:cat> <http://www.w3.org/2000/01/rdf-schema#label> \"Furry kitten purring\" .".as_slice(),
        oxrdfio::RdfFormat::NTriples, None, ts, None, None, graph).unwrap();
    let payload = quipu::share::share_payload(
        &producer,
        &ShareOptions {
            scope: ShareScope::Graph("urn:fixture:repository".into()),
            no_shapes: true,
            ..Default::default()
        },
        SHARE_PAYLOAD_MAX_BYTES,
    )
    .unwrap();
    let request: quipu::share_import::ShareImportRequest =
        serde_json::from_value(serde_json::json!({
            "manifest":payload.manifest, "export_ntriples":payload.files["export.nt"],
            "shapes_turtle":payload.files["shapes.ttl"]
        }))
        .unwrap();
    let config = quipu::EmbeddingConfig {
        model_path: Some(PathBuf::from(std::env::var("QUIPU_TEST_MODEL").unwrap())),
        tokenizer_path: Some(PathBuf::from(
            std::env::var("QUIPU_TEST_TOKENIZER").unwrap(),
        )),
        ..Default::default()
    };
    for (provider, enabled, expected) in [
        (true, true, "ready"),
        (true, false, "disabled"),
        (false, true, "unavailable"),
    ] {
        let mut store = Store::open_in_memory().unwrap();
        let mut config = config.clone();
        config.auto_embed = enabled;
        if provider {
            quipu::embedding_runtime::configure_provider(&mut store, &config).unwrap();
        }
        store.set_defer_auto_embed(true);
        let shared = Arc::new(super::super::StoreHandle::writer_only(store));
        let response = super::super::publication::import_share(
            State(shared.clone()),
            None,
            axum::Json(request.clone()),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response["embedding"]["state"], expected, "{response}");
        assert_eq!(
            response["embedding"]["indexed"],
            if expected == "ready" { 1 } else { 0 }
        );
        let store = shared.lock();
        assert_eq!(
            store.vector_count().unwrap(),
            if expected == "ready" { 1 } else { 0 }
        );
        assert!(
            store
                .entity_facts(store.lookup("urn:fixture:cat").unwrap().unwrap())
                .unwrap()
                .is_empty()
        );
    }
}
