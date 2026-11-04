//! In-memory comment-cohort search, with a retained measured base response.
use quipu::{Store, embedding::EmbeddingProvider, tool_search};
use serde_json::{Value, json};
use std::sync::Arc;

struct Aligned;
impl EmbeddingProvider for Aligned {
    fn embed_text(&self, _: &str) -> quipu::Result<Vec<f32>> {
        Ok(vec![1.0, 0.0])
    }
    fn dimension(&self) -> usize {
        2
    }
}
fn show(case: &str, result: &Value) {
    println!(
        "{}",
        json!({"case":case,"outcome":result["context"]["outcome"],
        "coverage":result["context"]["top_term_coverage"],
        "similarity":result["results"][0]["similarity"],
        "text":result["results"][0]["text"]})
    );
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let baseline: Value = serde_json::from_str(include_str!(
        "../docs/demos/comment-cohort/base-result.json"
    ))?;
    show("retained base21547f5a response (not rerun)", &baseline);
    let mut store = Store::open_in_memory()?;
    store.set_embedding_provider(Arc::new(Aligned));
    store.embedding_config_mut().auto_embed = true;
    let ttl = b"@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
        <https://example.invalid/subject/one> rdfs:label \"frobnix\" ;
        rdfs:comment \"automation service\", \"container management\", \"inventory fixture\" .";
    quipu::rdf::ingest_rdf(
        &mut store,
        &ttl[..],
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
    )?;
    let query = "frobnix automation service container management inventory fixture";
    let result = tool_search(&store, &json!({"query":query,"verbose":true}))?;
    show("candidate same input", &result);
    assert_eq!(result["context"]["top_term_coverage"], 1.0);
    assert_eq!(result["insufficient_context"], false);
    let unrelated = tool_search(
        &store,
        &json!({"query":"unrelated subject service","verbose":true}),
    )?;
    show("unrelated subject control", &unrelated);
    assert_eq!(unrelated["insufficient_context"], true);
    Ok(())
}
