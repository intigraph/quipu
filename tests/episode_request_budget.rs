//! Batch write deadlines must be meaningful and refusal must remain atomic.
use quipu::{
    Store,
    embedding::EmbeddingProvider,
    time::{Deadline, set_request_deadline},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
struct Slow(Arc<AtomicUsize>);
impl EmbeddingProvider for Slow {
    fn embed_text(&self, _: &str) -> quipu::error::Result<Vec<f32>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(5));
        Ok(vec![1.0, 0.0, 0.0])
    }
    fn dimension(&self) -> usize {
        3
    }
}
fn store() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let shapes = format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:document-shape> a sh:NodeShape ; sh:targetClass <{}Document> .",
        store.base_ns()
    );
    quipu::mcp::tools::tool_shapes(
        &mut store,
        &serde_json::json!({"action":"load", "name":"documents", "turtle":shapes}),
    )
    .unwrap();
    store
}
fn episode(count: usize) -> serde_json::Value {
    let nodes: Vec<_> = (0..count).map(|i| serde_json::json!({"name":format!("document-{i}"), "type":"Document", "description":"batch control", "properties":{"ordinal":i, "category":"control", "status":"committed"}})).collect();
    serde_json::json!({"name":format!("batch-{count}"), "nodes":nodes,"edges":[]})
}
#[test]
fn expired_episode_stops_resolution_and_keeps_store_unchanged() {
    let mut store = store();
    store.resolution_config_mut().enabled = true;
    let calls = Arc::new(AtomicUsize::new(0));
    store.set_embedding_provider(Arc::new(Slow(calls.clone())));
    let before = store.current_facts().unwrap();
    let deadline = set_request_deadline(Some(Deadline::after_millis(1)));
    let error = quipu::mcp::tools::tool_episode(&mut store, &episode(57)).unwrap_err();
    drop(deadline);
    assert_eq!(
        store.current_facts().unwrap(),
        before,
        "refused write changed facts"
    );
    assert!(
        calls.load(Ordering::Relaxed) < 57,
        "resolution consumed the entire expired batch"
    );
    assert!(
        error.to_string().contains("request deadline"),
        "wrong budget diagnosis: {error}"
    );
    assert!(!error.to_string().contains("exceeded 0ms"));
    match error {
        quipu::Error::RequestTimeout {
            elapsed_ms,
            limit_ms,
        } => {
            assert_eq!(limit_ms, 1);
            assert!(elapsed_ms >= limit_ms);
        }
        other => panic!("expected original request budget, got {other:?}"),
    }
    // The ambient budget must not leak to the next write on this thread.
    store.resolution_config_mut().enabled = false;
    assert!(quipu::mcp::tools::tool_episode(&mut store, &episode(1)).is_ok());
}
#[test]
fn hundreds_of_typed_nodes_commit_hundreds_of_triples_within_budget() {
    for count in [50, 200] {
        let mut store = store();
        let before = store.current_facts().unwrap().len();
        let deadline = set_request_deadline(Some(Deadline::after_millis(5000)));
        let output = quipu::mcp::tools::tool_episode(&mut store, &episode(count)).unwrap();
        drop(deadline);
        let after = store.current_facts().unwrap().len();
        assert_eq!(output["outcome"], "created");
        assert!(after - before >= count * 6);
        // A loaded vocabulary is exercised, not merely bootstrap acceptance.
        let invalid = serde_json::json!({"name":"bad", "nodes":[{"name":"unknown", "type":"Unregistered"}], "edges":[]});
        assert!(quipu::mcp::tools::tool_episode(&mut store, &invalid).is_err());
        assert_eq!(store.current_facts().unwrap().len(), after);
    }
}
