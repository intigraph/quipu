//! Compare ordinary and opt-in query responses over a local in-memory fixture.
use quipu::{Store, tool_query};
use serde_json::json;

fn main() -> quipu::Result<()> {
    let store = Store::open_in_memory()?;
    let query = "SELECT ?x WHERE { VALUES ?x { 1 2 } FILTER(?x = 1) }";
    let ordinary = tool_query(&store, &json!({"query":query}))?;
    let mut profiled = tool_query(&store, &json!({"query":query,"execution_profile":true}))?;
    let profile = profiled
        .as_object_mut()
        .unwrap()
        .remove("execution_profile")
        .unwrap();
    assert_eq!(ordinary, profiled);
    println!("request: {query}");
    println!("ordinary: {}", serde_json::to_string(&ordinary).unwrap());
    println!("profile: {}", serde_json::to_string(&profile).unwrap());
    println!(
        "control: normal answer is identical; count={}",
        ordinary["count"]
    );
    Ok(())
}
