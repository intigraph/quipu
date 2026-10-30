//! Offline before/after reproducer for allocated key identity invariants.
use quipu::entity_namespaces::{Namespace, apply, configure};
use quipu::entity_reference::tool_entity_ref;
use quipu::{Op, QuipuConfig, Store, Value};
use serde_json::json;

const TYPE: &str = "urn:example:Endpoint";
const HOST: &str = "urn:example:host";
const PORT: &str = "urn:example:port";
const NOW: &str = "2026-10-30T00:00:00Z";

fn shapes(strategy: &str) -> String {
    format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> .
         @prefix q: <https://quipu.dev/ontology/> .
         @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
         <urn:example:Shape> a sh:NodeShape; sh:targetClass <{TYPE}>;
         q:keyFields (<{HOST}> <{PORT}>); q:keyStrategy \"{strategy}\";
         sh:property [sh:path <{HOST}>; sh:minCount 1; sh:maxCount 1; sh:datatype xsd:string];
         sh:property [sh:path <{PORT}>; sh:minCount 1; sh:maxCount 1; sh:datatype xsd:integer]."
    )
}

fn fixture() -> (Store, String) {
    let mut store = Store::open_in_memory().expect("isolated fixture");
    let mut config = QuipuConfig::default();
    config.namespaces.push(Namespace {
        prefix: "ep".into(),
        base_iri: "https://example.org/endpoints/".into(),
        minting: "key".into(),
        types: vec![TYPE.into()],
        ..Namespace::default()
    });
    configure(&mut store, &config).unwrap();
    apply(&mut store).unwrap();
    store.load_shapes("endpoint", &shapes("hash"), NOW).unwrap();
    let result = tool_entity_ref(
        &mut store,
        &json!({"action":"mint","namespace":"ep","type":TYPE,
            "keys":{HOST:"db.example",PORT:5432}}),
    )
    .unwrap();
    (store, result["iri"].as_str().unwrap().into())
}

fn main() {
    let guarded = std::env::args().any(|a| a == "--expect-guards");
    println!("Synthetic in-memory store; no network or installed database.");
    let (mut store, iri) = fixture();
    println!("Allocated: {iri} (host=db.example, port=5432)");
    let entity = store.lookup(&iri).unwrap().unwrap();
    let attribute = store.lookup(PORT).unwrap().unwrap();
    let datum = |value, op| quipu::store::Datum {
        entity,
        attribute,
        value: Value::Int(value),
        op,
        valid_from: NOW.into(),
        valid_to: None,
    };
    store
        .transact(&[datum(5432, Op::Assert)], NOW, None, None)
        .expect("identical assertion stays allowed");
    println!("Positive control: identical key assertion accepted");
    let before = store.latest_tx_id().unwrap();
    let result = store.transact(
        &[datum(5432, Op::Retract), datum(5433, Op::Assert)],
        NOW,
        None,
        None,
    );
    println!("Replace port 5432 -> 5433: accepted={}", result.is_ok());
    assert_eq!(result.is_err(), guarded);
    if guarded {
        assert_eq!(store.latest_tx_id().unwrap(), before);
        println!("Refusal leaves transaction head unchanged");
    }
    let (mut store, iri) = fixture();
    let result = store.load_shapes("endpoint", &shapes("lexical"), NOW);
    println!(
        "Change allocated strategy hash -> lexical: accepted={}",
        result.is_ok()
    );
    assert_eq!(result.is_err(), guarded);
    if guarded {
        let before = store.latest_tx_id().unwrap();
        let result = tool_entity_ref(&mut store, &json!({"action":"verify","iri":iri})).unwrap();
        println!(
            "Verify unchanged reference: verified={}",
            result["verified"]
        );
        assert_eq!(store.latest_tx_id().unwrap(), before);
    }
}
