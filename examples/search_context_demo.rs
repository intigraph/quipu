//! Source-only in-memory diagnostic versus admitted-context demonstration.
use quipu::{Store, tool_search, vector::KnowledgeVectorStore};
use serde_json::json;

fn main() -> quipu::Result<()> {
    let store = Store::open_in_memory()?;
    let entity = store.intern("urn:memory")?;
    store.embed_entity(
        entity,
        "memory scheduler allocation",
        &[1.0, 0.0],
        "2026-01-01",
    )?;
    for (label, query) in [
        (
            "missing subject",
            concat!("absent", "subject memory scheduler"),
        ),
        ("present subject", "memory scheduler allocation"),
    ] {
        let result = tool_search(&store, &json!({"query":query,"embedding":[1.0,0.0]}))?;
        println!(
            "{}",
            json!({"case":label,"diagnostic_count":result["results"].as_array().unwrap().len(),
            "context_count":result["context_results"].as_array().unwrap().len(),
            "outcome":result["context"]["outcome"],"reason":result["context"]["reason"]})
        );
    }
    Ok(())
}
