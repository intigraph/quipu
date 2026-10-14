use quipu::{
    Store,
    embedding::EmbeddingProvider,
    time::{Deadline, set_request_deadline},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
struct Slow;
impl EmbeddingProvider for Slow {
    fn embed_text(&self, _: &str) -> quipu::error::Result<Vec<f32>> {
        std::thread::sleep(Duration::from_millis(2));
        Ok(vec![1.0, 0.0, 0.0])
    }
    fn dimension(&self) -> usize {
        3
    }
}
fn main() {
    let mut misleading = false;
    for count in [1, 19, 57] {
        let mut store = Store::open_in_memory().unwrap();
        store.resolution_config_mut().enabled = true;
        store.set_embedding_provider(Arc::new(Slow));
        let nodes: Vec<_> = (0..count).map(|i| serde_json::json!({"name":format!("private-document-{i}"),"type":"Document","description":"synthetic diagnostic"})).collect();
        let input =
            serde_json::json!({"name":format!("private-probe-{count}"),"nodes":nodes,"edges":[]});
        let before = store.current_facts().unwrap().len();
        let started = Instant::now();
        let deadline = set_request_deadline(Some(Deadline::after_millis(20)));
        let result = quipu::mcp::tools::tool_episode(&mut store, &input);
        drop(deadline);
        let after = store.current_facts().unwrap().len();
        println!(
            "nodes={count} elapsed_ms={} before={before} after={after} result={result:?}",
            started.elapsed().as_millis()
        );
        if count == 1 {
            assert!(result.is_ok(), "one-node positive control");
        }
        if result.is_err() {
            assert_eq!(before, after, "refusal atomicity");
        }
        if let Err(e) = result {
            misleading |= e.to_string().contains("exceeded 0ms");
        }
    }
    assert!(
        !misleading,
        "request budget exhaustion must not be reported as zero configured budget"
    );
}
