use super::*;
use crate::share::{SHARE_PAYLOAD_MAX_BYTES, ShareOptions, ShareScope, share_payload};
use crate::share_import::{ShareImportRequest, import_share};
use crate::vector::KnowledgeVectorStore;

const TS: &str = "2026-10-20T00:00:00Z";

fn source() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut store);
    let graph = store.graph_create("urn:fixture:repository").unwrap();
    crate::rdf::ingest_rdf_to_graph(&mut store, &br#"
      <urn:fixture:cat> <http://www.w3.org/2000/01/rdf-schema#label> "Architecture note A" ;
        <http://www.w3.org/2000/01/rdf-schema#comment> "A furry kitten purrs, chases mice and lives in a feline shelter" ;
        <urn:fixture:filePath> "docs/feline-design.md" .
      <urn:fixture:car> <http://www.w3.org/2000/01/rdf-schema#label> "Architecture note B" ;
        <http://www.w3.org/2000/01/rdf-schema#comment> "A motor vehicle engine drives an automobile on highways" ;
        <urn:fixture:filePath> "docs/vehicle-design.md" .
    "#[..], oxrdfio::RdfFormat::Turtle, None, TS, None, None, graph).unwrap();
    store
}

fn request(store: &Store) -> ShareImportRequest {
    let payload = share_payload(
        store,
        &ShareOptions {
            scope: ShareScope::Graph("urn:fixture:repository".into()),
            no_shapes: true,
            ..Default::default()
        },
        SHARE_PAYLOAD_MAX_BYTES,
    )
    .unwrap();
    serde_json::from_value(serde_json::json!({
        "manifest": payload.manifest, "export_ntriples": payload.files["export.nt"],
        "shapes_turtle": payload.files["shapes.ttl"], "source":"private embedding control"
    }))
    .unwrap()
}

#[test]
fn omitted_setting_is_on_and_explicit_false_survives_deserialization() {
    let omitted: crate::EmbeddingConfig = toml::from_str("").unwrap();
    assert!(omitted.auto_embed);
    let disabled: crate::EmbeddingConfig = toml::from_str("auto_embed = false").unwrap();
    assert!(!disabled.auto_embed);
    assert!(
        Store::open_in_memory()
            .unwrap()
            .embedding_config()
            .auto_embed
    );
}

#[test]
fn missing_provider_reports_graph_staged_not_search_ready() {
    let req = request(&source());
    let mut receiver = Store::open_in_memory().unwrap();
    let result = import_share(&mut receiver, &req, TS, None).unwrap();
    assert_eq!(result.embedding.state, "unavailable");
    assert!(
        result
            .embedding
            .message
            .unwrap()
            .contains("No embedding model")
    );
    assert_eq!(result.embedding.indexed, 0);
    assert_eq!(receiver.vector_count().unwrap(), 0);
    assert!(
        receiver
            .entity_facts(receiver.lookup("urn:fixture:cat").unwrap().unwrap())
            .unwrap()
            .is_empty()
    );
    assert_eq!(result.triples.accepted, 6);
}

#[test]
fn opt_out_preserves_import_and_does_not_claim_index_ready() {
    let mut receiver = Store::open_in_memory().unwrap();
    receiver.embedding_config_mut().auto_embed = false;
    let result = import_share(&mut receiver, &request(&source()), TS, None).unwrap();
    assert_eq!(result.embedding.state, "disabled");
    assert_eq!(receiver.vector_count().unwrap(), 0);
    assert_eq!(result.triples.accepted, 6);
}

#[test]
fn malformed_recipe_refuses_before_a_staging_graph_exists() {
    let mut req = request(&source());
    req.manifest.embedding = Some(EmbeddingRecipe {
        model: "model.onnx".into(),
        model_sha256: "invalid".into(),
        tokenizer_sha256: "0".repeat(64),
        dimension: 384,
    });
    req.manifest.share_id =
        crate::share::sha256(&crate::share::manifest_bytes(&req.manifest, false).unwrap());
    let mut receiver = Store::open_in_memory().unwrap();
    let before = receiver.latest_tx_id().unwrap();
    assert!(
        import_share(&mut receiver, &req, TS, None)
            .unwrap_err()
            .to_string()
            .contains("invalid pendant embedding recipe")
    );
    assert_eq!(receiver.latest_tx_id().unwrap(), before);
    assert_eq!(receiver.vector_count().unwrap(), 0);
}

/// Acceptance uses the real supported ONNX provider, not a similarity stub.
#[cfg(all(feature = "onnx", feature = "mcp"))]
#[test]
#[ignore = "requires QUIPU_TEST_MODEL and QUIPU_TEST_TOKENIZER pointing to a real local sentence-transformer"]
fn actual_onnx_import_replay_deferred_optout_mismatch_and_scoped_search() {
    use crate::EmbeddingProvider;
    use std::sync::Arc;
    let model =
        std::path::PathBuf::from(std::env::var("QUIPU_TEST_MODEL").expect("real model required"));
    let tokenizer = std::path::PathBuf::from(
        std::env::var("QUIPU_TEST_TOKENIZER").expect("real tokenizer required"),
    );
    let provider =
        Arc::new(crate::OnnxEmbeddingProvider::load(&model, &tokenizer, 384, 256).unwrap());
    let mut producer = source();
    producer.set_embedding_provider(provider.clone());
    let req = request(&producer);
    assert_eq!(req.manifest.embedding, provider.recipe());
    let mut receiver = Store::open_in_memory().unwrap();
    receiver.set_embedding_provider(provider.clone());
    let first = import_share(&mut receiver, &req, TS, None).unwrap();
    assert_eq!(first.embedding.state, "ready");
    assert_eq!(first.embedding.indexed, 2);
    assert_eq!(receiver.vector_count().unwrap(), 2);
    let query = provider.embed_text("a pet cat playing").unwrap();
    let root = crate::mcp::tools::tool_search(
        &receiver,
        &serde_json::json!({"embedding":query,"graph":"urn:quipu:graph:root","verbose":true}),
    )
    .unwrap();
    assert!(
        root["results"].as_array().unwrap().is_empty(),
        "staged vectors must not leak into ROOT search"
    );
    receiver.search_config_mut().named_graphs = true;
    let scoped = crate::mcp::tools::tool_search(&receiver,&serde_json::json!({"embedding":query,"graph":first.staging_graph,"limit":1,"verbose":true})).unwrap();
    assert_eq!(scoped["results"][0]["entity"], "urn:fixture:cat");
    let again = import_share(&mut receiver, &req, TS, None).unwrap();
    assert_eq!(again.outcome, "unchanged");
    assert_eq!(again.embedding.indexed, 2);
    assert_eq!(receiver.vector_count().unwrap(), 2);

    let mut deferred = Store::open_in_memory().unwrap();
    deferred.set_embedding_provider(provider.clone());
    deferred.set_defer_auto_embed(true);
    let mut result = import_share(&mut deferred, &req, TS, None).unwrap();
    assert_eq!(result.embedding.state, "pending");
    assert_eq!(deferred.vector_count().unwrap(), 0);
    let work = deferred.take_deferred_embed().unwrap();
    let embeddings = provider.embed_batch(&work.texts()).unwrap();
    assert!(
        deferred
            .apply_deferred_embed(&work, &embeddings[..1])
            .is_err()
    );
    assert_eq!(deferred.vector_count().unwrap(), 0);
    assert_eq!(
        deferred.apply_deferred_embed(&work, &embeddings).unwrap(),
        2
    );
    refresh(&deferred, &mut result).unwrap();
    assert_eq!(result.embedding.state, "ready");

    // An unchanged graph with old vectors is still pending during a fresh
    // provider drain, never reported ready from text equality alone.
    receiver.set_defer_auto_embed(true);
    let replay = import_share(&mut receiver, &req, TS, None).unwrap();
    assert_eq!(replay.embedding.state, "pending");
    assert_eq!(replay.embedding.indexed, 0);

    // A ROOT write arriving while inference is outside the lock owns its
    // entity. Staged text must never overwrite the newer ROOT vector.
    let work = deferred.take_deferred_embed();
    assert!(work.is_none());
    let mut race = import_share(&mut deferred, &req, TS, None).unwrap();
    let work = deferred.take_deferred_embed().unwrap();
    let embeddings = provider.embed_batch(&work.texts()).unwrap();
    deferred.embedding_config_mut().auto_embed = false;
    crate::rdf::ingest_rdf(&mut deferred,
        &b"<urn:fixture:cat> <http://www.w3.org/2000/01/rdf-schema#label> \"Adopted description\" ."[..],
        oxrdfio::RdfFormat::NTriples, None, TS, None, None).unwrap();
    assert_eq!(
        deferred.apply_deferred_embed(&work, &embeddings).unwrap(),
        1
    );
    refresh(&deferred, &mut race).unwrap();
    assert_eq!(race.embedding.state, "partial");
    assert_eq!(race.embedding.skipped_shared_entities, 1);

    let mut disabled = Store::open_in_memory().unwrap();
    disabled.set_embedding_provider(provider.clone());
    disabled.embedding_config_mut().auto_embed = false;
    let result = import_share(&mut disabled, &req, TS, None).unwrap();
    assert_eq!(result.embedding.state, "disabled");
    assert_eq!(disabled.vector_count().unwrap(), 0);

    let mut wrong = req.clone();
    wrong.manifest.embedding.as_mut().unwrap().model_sha256 = "0".repeat(64);
    wrong.manifest.share_id =
        crate::share::sha256(&crate::share::manifest_bytes(&wrong.manifest, false).unwrap());
    let mut untouched = Store::open_in_memory().unwrap();
    untouched.set_embedding_provider(provider.clone());
    assert!(
        import_share(&mut untouched, &wrong, TS, None)
            .unwrap_err()
            .to_string()
            .contains("does not match")
    );
    assert_eq!(untouched.latest_tx_id().unwrap(), 0);
    assert!(untouched.lookup("urn:fixture:cat").unwrap().is_none());
    assert!(
        untouched.embedding_config().auto_embed,
        "error must restore the configured default"
    );
    untouched.embedding_config_mut().auto_embed = false;
    assert!(
        import_share(&mut untouched, &wrong, TS, None).is_err(),
        "opt-out does not attest mismatched provider bytes"
    );
    assert!(!untouched.embedding_config().auto_embed);
    assert_eq!(untouched.latest_tx_id().unwrap(), 0);
    assert!(
        crate::OnnxEmbeddingProvider::load(&model, &tokenizer, 128, 256).is_err(),
        "wrong configured dimension must fail at provider setup"
    );
}

#[test]
fn provider_failure_is_reported_separately_from_durable_staging() {
    struct Broken;
    impl crate::EmbeddingProvider for Broken {
        fn embed_text(&self, _: &str) -> crate::Result<Vec<f32>> {
            Err(crate::Error::Store("synthetic inference failure".into()))
        }
        fn dimension(&self) -> usize {
            384
        }
    }
    let mut receiver = Store::open_in_memory().unwrap();
    receiver.set_embedding_provider(std::sync::Arc::new(Broken));
    let result = import_share(&mut receiver, &request(&source()), TS, None).unwrap();
    assert_eq!(result.embedding.state, "failed");
    assert_eq!(result.embedding.indexed, 0);
    assert!(
        result
            .embedding
            .message
            .unwrap()
            .contains("synthetic inference failure")
    );
    assert_eq!(result.triples.accepted, 6);
    assert_eq!(receiver.vector_count().unwrap(), 0);
    assert!(receiver.latest_tx_id().unwrap() > 0);
}
