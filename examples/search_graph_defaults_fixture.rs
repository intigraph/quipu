//! Isolated search-default migration and old-reader fixture; no network calls.
use quipu::Store;
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 || !matches!(args[1].as_str(), "existing" | "new") {
        return Err("usage: search_graph_defaults_fixture existing|new OUTPUT_DB".into());
    }
    let path = std::path::Path::new(&args[2]);
    if path.exists() {
        return Err("refusing to overwrite an existing fixture".into());
    }
    if args[1] == "existing" {
        let old = rusqlite::Connection::open(path)?;
        old.execute_batch(include_str!(
            "../src/search_graph_scope/fixtures/pre_scope_defaults.sql"
        ))?;
    }
    let mut store = Store::open(path.to_str().ok_or("non-UTF8 fixture path")?)?;
    store.search_config_mut().keyword = true;
    store.initialize_lexical_index()?;
    for (graph, entity, name) in [
        (None, "urn:root", "shared root"),
        (Some("urn:graph:project"), "urn:named", "shared named"),
    ] {
        let text = format!("<{entity}> <http://www.w3.org/2000/01/rdf-schema#label> \"{name}\" .");
        if let Some(graph) = graph {
            let id = store.graph_create(graph)?;
            quipu::rdf::ingest_rdf_to_graph(
                &mut store,
                text.as_bytes(),
                oxrdfio::RdfFormat::Turtle,
                None,
                "2026-01-01T00:00:00Z",
                None,
                None,
                id,
            )?;
        } else {
            quipu::rdf::ingest_rdf(
                &mut store,
                text.as_bytes(),
                oxrdfio::RdfFormat::Turtle,
                None,
                "2026-01-01T00:00:00Z",
                None,
                None,
            )?;
        }
    }
    store.backfill_lexical_batch(100)?;
    store.vector_store().embed_entity(
        store.lookup("urn:root")?.ok_or("missing ROOT control")?,
        "shared root",
        &[0.8, 0.2],
        "2026-01-01T00:00:00Z",
    )?;
    let input = json!({"embedding":[1.0,0.0],"verbose":true,"limit":1});
    let keyword = quipu::tool_search(&store, &json!({"query":"shared","mode":"keyword"}))?;
    let unprepared = match quipu::tool_search(&store, &input) {
        Ok(value) => json!({"result":value}),
        Err(error) => json!({"refused":error.to_string()}),
    };
    // This is a two-entity fixture, not a production backfill or readiness claim.
    store.vector_store().embed_entity(
        store.lookup("urn:named")?.ok_or("missing named control")?,
        "shared named",
        &[1.0, 0.0],
        "2026-01-01T00:00:00Z",
    )?;
    store.search_config_mut().named_graphs = true;
    let prepared = quipu::tool_search(&store, &input)?;
    store.search_config_mut().default_graph_scope = Some("all".into());
    let explicit_all = quipu::tool_search(&store, &input)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "fixture_kind":args[1], "keyword":keyword,
            "unprepared_semantic":unprepared, "prepared_default":prepared,
            "explicit_all":explicit_all,
            "limit":"isolated fixture; no installed-service or production vector readiness proof"
        }))?
    );
    Ok(())
}
