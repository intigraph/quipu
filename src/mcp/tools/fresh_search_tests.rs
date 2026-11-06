use super::*;
use crate::embedding::EmbeddingProvider;
use crate::vector::KnowledgeVectorStore;
use std::sync::Arc;

fn fixture() -> Store {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    let mut ttl = String::new();
    for i in 0..250 {
        ttl.push_str(&format!(
            "<urn:filler:{i}> <urn:label> \"unrelated {i}\" .\n"
        ));
    }
    ttl.push_str("<urn:orchid> <http://www.w3.org/2000/01/rdf-schema#label> \"OrchidLedger\" .\n");
    crate::rdf::ingest_rdf(
        &mut store,
        ttl.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    store
}

#[test]
fn ejzx_zero_embeddings_all_four_tools_find_a_late_label() {
    let store = fixture();
    let q = json!({"query":"OrchidLedger","limit":5,"max_results":5});
    for tool in [
        super::super::search::tool_search,
        super::super::search::tool_hybrid_search,
        crate::mcp::search::tool_search_nodes,
        crate::mcp::search::tool_search_facts,
    ] {
        let out = tool(&store, &q).unwrap();
        assert!(out["count"].as_u64().unwrap() > 0, "{out}");
        assert_eq!(out["retrieval"]["mode"], "lexical");
        assert_eq!(out["retrieval"]["embeddings"], 0);
        assert_eq!(out["retrieval"]["status"], "degraded");
    }
}

#[test]
fn ejzx_lexical_never_fabricates_semantic_context_or_cosines() {
    let out =
        super::super::search::tool_search(&fixture(), &json!({"query":"OrchidLedger"})).unwrap();
    assert!(out.get("context").is_none());
    assert!(out["results"][0].get("similarity").is_none());
    assert_eq!(out["results"][0]["evidence_mode"], "lexical");
    assert_eq!(out["retrieval"]["contract"], "quipu-lexical-results/v1");
}

#[test]
fn ejzx_fallback_keeps_sparql_candidates_and_invalid_graph_refusals() {
    let store = fixture();
    let out = super::super::search::tool_hybrid_search(
        &store,
        &json!({"query":"OrchidLedger",
        "sparql":"SELECT ?s WHERE { ?s <urn:not-recorded> ?o }"}),
    )
    .unwrap();
    assert_eq!(out["count"], 0);
    assert_eq!(out["retrieval"]["status"], "degraded");
    assert!(
        super::super::search::tool_search(
            &store,
            &json!({"query":"OrchidLedger","graph":"urn:unknown"})
        )
        .is_err()
    );
}

#[test]
fn ejzx_explicit_vectors_and_disabled_search_modes_keep_their_contract() {
    let store = fixture();
    assert!(
        super::super::search::tool_search(
            &store,
            &json!({"query":"OrchidLedger","mode":"keyword"})
        )
        .is_err()
    );
    assert!(super::super::search::tool_search(&store, &json!({"embedding":[1,0]})).is_ok());
    for query in ["", "   "] {
        assert!(super::super::search::tool_search(&store, &json!({"query":query})).is_err());
    }
}

struct Provider;
impl EmbeddingProvider for Provider {
    fn embed_text(&self, _: &str) -> Result<Vec<f32>> {
        Ok(vec![1.0, 0.0])
    }
    fn dimension(&self) -> usize {
        2
    }
}

#[test]
fn ejzx_partial_index_keeps_admitted_vectors_and_unindexed_lexical_rows() {
    let mut store = fixture();
    crate::rdf::ingest_rdf(&mut store,
        &b"<urn:vector-orchid> <http://www.w3.org/2000/01/rdf-schema#label> \"OrchidLedger vector evidence\" ."[..],
        oxrdfio::RdfFormat::Turtle, None, "2026-01-01T00:00:00Z", None, None).unwrap();
    let id = store.intern("urn:vector-orchid").unwrap();
    store
        .embed_entity(
            id,
            "OrchidLedger vector evidence",
            &[1.0, 0.0],
            "2026-01-01",
        )
        .unwrap();
    store.set_embedding_provider(Arc::new(Provider));
    let out =
        super::super::search::tool_search(&store, &json!({"query":"OrchidLedger","limit":10}))
            .unwrap();
    assert_eq!(out["retrieval"]["mode"], "mixed", "{out}");
    assert!(
        out["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["evidence_mode"] == "lexical")
    );
    assert!(
        out["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["similarity"].as_f64() == Some(1.0))
    );
}

#[test]
fn ejzx_configured_provider_still_embeds_writes_without_backfill() {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    store.set_embedding_provider(Arc::new(Provider));
    assert!(store.embedding_config().auto_embed);
    crate::rdf::ingest_rdf(
        &mut store,
        &b"<urn:orchid> <http://www.w3.org/2000/01/rdf-schema#label> \"OrchidLedger\" ."[..],
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    assert!(store.vector_store().vector_count().unwrap() > 0);
    let out = super::super::search::tool_search(&store, &json!({"query":"OrchidLedger"})).unwrap();
    assert_eq!(out["retrieval"]["mode"], "semantic", "{out}");
    assert_eq!(out["insufficient_context"], false, "{out}");
}

#[test]
fn ejzx_expired_deadline_is_an_error_not_a_false_empty_answer() {
    let store = fixture();
    let _guard = crate::time::set_request_deadline(Some(crate::time::Deadline::after_millis(0)));
    assert!(super::super::search::tool_search(&store, &json!({"query":"OrchidLedger"})).is_err());
}

#[test]
fn ejzx_integrated_auto_keyword_backend_retains_main_fields() {
    let mut store = fixture();
    store.search_config_mut().keyword = true;
    store.search_config_mut().mode = "auto".into();
    store.initialize_lexical_index().unwrap();
    while !store.backfill_lexical_batch(100).unwrap().complete {}
    let q = json!({"query":"OrchidLedger"});
    let out = super::super::search::tool_search(&store, &q).unwrap();
    assert_eq!(out["count"], 1);
    assert_eq!(out["ranking"], "keyword");
    assert_eq!(out["mode_reason"], "no_embedding_provider");
    assert_eq!(out["retrieval"]["mode"], "lexical");
    assert_eq!(out["retrieval"]["status"], "degraded");
    assert_eq!(out["results"][0]["evidence_mode"], "lexical");
    assert!(out["results"][0].get("similarity").is_none());
    assert!(out.get("context").is_none());
    let explicit = super::super::search::tool_search(
        &store,
        &json!({"query":"OrchidLedger","mode":"semantic"}),
    );
    assert!(explicit.is_err());
}
