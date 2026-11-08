//! In-memory explicit-address demonstration; no server or deployment writes.
use quipu::{Store, sparql, tool_episode, tool_shapes};
use serde_json::json;

fn main() -> quipu::Result<()> {
    for explicit in [false, true] {
        let mut store = Store::open_in_memory()?;
        store.set_base_ns("https://example.org/legacy/");
        tool_shapes(
            &mut store,
            &json!({"action":"load", "name":"demo-types",
            "turtle":"@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:demo:shape> a sh:NodeShape; sh:targetClass <https://example.org/legacy/Host> ."}),
        )?;
        let mut input = json!({"name":"address-demo", "source":"synthetic fixture",
            "nodes":[{"name":"worker", "type":"Host"}]});
        if explicit {
            input["nodes"][0]["iri"] = json!("urn:example:worker");
        }
        let output = tool_episode(&mut store, &input)?;
        println!("explicit={explicit}: outcome={}", output["outcome"]);
        for iri in ["urn:example:worker", "https://example.org/legacy/worker"] {
            let present = matches!(
                sparql::query(&store, &format!("ASK {{ <{iri}> a ?t }}"))?,
                sparql::QueryResult::Ask(true)
            );
            println!("  {iri}: present={present}");
        }
        if explicit {
            input["name"] = json!("bad-address-demo");
            input["nodes"][0]["iri"] = json!("relative/path");
            let before = store.latest_tx_id()?;
            let refused = tool_episode(&mut store, &input).is_err();
            println!(
                "  malformed address refused={refused}; transaction unchanged={}",
                before == store.latest_tx_id()?
            );
        }
    }
    Ok(())
}
