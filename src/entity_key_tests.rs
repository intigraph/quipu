use super::*;
use crate::Store;
use crate::entity_namespaces::{Namespace, apply, configure};
use crate::entity_reference::tool_entity_ref;
use serde_json::json;

const ENDPOINT: &str = "urn:example:Endpoint";
const HOST: &str = "urn:example:host";
const PORT: &str = "urn:example:port";

const XSD_STRING: &str = crate::namespace::XSD_STRING;
const XSD_INTEGER: &str = crate::namespace::XSD_INTEGER;

fn shapes(extra: &str, max: &str) -> String {
    shapes_kinds(
        extra,
        max,
        &format!("sh:datatype <{XSD_STRING}>"),
        &format!("sh:datatype <{XSD_INTEGER}>"),
    )
}

fn shapes_kinds(extra: &str, max: &str, host_kind: &str, port_kind: &str) -> String {
    format!(
        r#"@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix quipu: <https://quipu.dev/ontology/> .
<urn:example:EndpointShape> a sh:NodeShape ;
  sh:targetClass <{ENDPOINT}> ;
  quipu:keyFields ( <{HOST}> <{PORT}> ) ;
  {extra}
  sh:property [ sh:path <{HOST}> ; sh:minCount 1 ; sh:maxCount 1 ; {host_kind} ] ;
  sh:property [ sh:path <{PORT}> ; sh:minCount 1 ; sh:maxCount {max} ; {port_kind} ] .
"#
    )
}

fn store_with(minting_types: &[&str], shapes_ttl: &str) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let mut cfg = crate::QuipuConfig::default();
    cfg.namespaces.push(Namespace {
        prefix: "ep".into(),
        base_iri: "https://public.example/ep/".into(),
        minting: "key".into(),
        types: minting_types.iter().map(|t| (*t).to_string()).collect(),
        ..Namespace::default()
    });
    configure(&mut store, &cfg).unwrap();
    apply(&mut store).unwrap();
    store
        .load_shapes("endpoint", shapes_ttl, &crate::time::now_iso())
        .unwrap();
    store
}

fn mint(store: &mut Store, keys: &serde_json::Value) -> crate::Result<serde_json::Value> {
    tool_entity_ref(
        store,
        &json!({"action":"mint","namespace":"ep","type":ENDPOINT,"keys":keys}),
    )
}

#[test]
fn two_writers_converge_on_one_iri_and_labels_are_not_key_material() {
    let ttl = shapes("", "1");
    let mut a = store_with(&[ENDPOINT], &ttl);
    let mut b = store_with(&[ENDPOINT], &ttl);
    let keys = json!({HOST:"db.lan", PORT:5432});
    let first = mint(&mut a, &keys).unwrap();
    assert_eq!(first["converged"], false);
    let other_writer = tool_entity_ref(
        &mut b,
        &json!({"action":"mint","namespace":"ep","type":ENDPOINT,"keys":keys,"label":"primary db"}),
    )
    .unwrap();
    assert_eq!(
        first["iri"], other_writer["iri"],
        "independent stores derive one IRI"
    );
    // A repeat in the same store converges: same IRI, no transaction.
    let facts = a.current_facts().unwrap().len();
    let again = mint(&mut a, &json!({PORT:5432, HOST:"db.lan"})).unwrap();
    assert_eq!(again["iri"], first["iri"]);
    assert_eq!(again["converged"], true);
    assert!(again["tx_id"].is_null());
    assert_eq!(a.current_facts().unwrap().len(), facts);
    // The key facts were written, so the minted node satisfies its own shape.
    let id = a.lookup(first["iri"].as_str().unwrap()).unwrap().unwrap();
    assert_eq!(a.entity_facts(id).unwrap().len(), 3);
}

#[test]
fn hash_derivation_is_pinned() {
    // Golden vector: changing this breaks every key IRI ever minted. A new
    // derivation needs a new HASH_VERSION, not an edit to this expectation.
    let spec = KeySpec {
        fields: vec![HOST.into(), PORT.into()],
        kinds: vec![KeyKind::String, KeyKind::Integer],
        strategy: Strategy::Hash,
    };
    let keys = json!({HOST:"db.lan", PORT:5432});
    let terms = key_terms(&spec, keys.as_object().unwrap()).unwrap();
    let key = derive_key(&spec, ENDPOINT, &terms);
    assert_eq!(key.len(), 26);
    assert_eq!(key, GOLDEN);
}

// Independently reproducible: base32(sha256("quipu-key-v1\n<urn:example:Endpoint>\n
// <urn:example:host>\n\"db.lan\"\n<urn:example:port>\n\"5432\"^^<xsd:integer>\n")[..16]),
// with the xsd IRI spelled in full (checked with Python hashlib/base64).
const GOLDEN: &str = "fto6n7scclh54e6w5hi55ah3bm";

#[test]
fn a_changed_key_value_or_type_is_a_different_entity() {
    let mut store = store_with(&[ENDPOINT], &shapes("", "1"));
    let a = mint(&mut store, &json!({HOST:"db.lan", PORT:5432})).unwrap();
    let moved = mint(&mut store, &json!({HOST:"db2.lan", PORT:5432})).unwrap();
    assert_ne!(
        a["iri"], moved["iri"],
        "a new host is a new endpoint, linked by supersession"
    );
    let spec = KeySpec {
        fields: vec![HOST.into(), PORT.into()],
        kinds: vec![KeyKind::String, KeyKind::Integer],
        strategy: Strategy::Hash,
    };
    let keys = json!({HOST:"db.lan", PORT:5432});
    let terms = key_terms(&spec, keys.as_object().unwrap()).unwrap();
    assert_ne!(
        derive_key(&spec, ENDPOINT, &terms),
        derive_key(&spec, "urn:example:Socket", &terms)
    );
    // "5432" the string cannot mint where the shape declares an integer port,
    // so 5432 and "5432" never become two IRIs for one endpoint.
    assert!(mint(&mut store, &json!({HOST:"db.lan", PORT:"5432"})).is_err());
}

#[test]
fn malformed_keys_refuse() {
    let mut store = store_with(&[ENDPOINT], &shapes("", "1"));
    for keys in [
        json!({HOST:"db.lan"}),
        json!({HOST:"db.lan", PORT:5432, "urn:example:extra":"x"}),
        json!({HOST:"db.lan", PORT:5432.5}),
        json!({HOST:"", PORT:5432}),
        json!({HOST:"db.lan", PORT:{"@value":"05432","@type":crate::namespace::XSD_INTEGER}}),
        json!({HOST:"db.lan", PORT:{"@value":"1.0","@type":crate::namespace::XSD_DOUBLE}}),
        json!({HOST:["db.lan"], PORT:5432}),
        json!({HOST:{"@id":"urn:host:db"}, PORT:5432}),
        json!({HOST:{"@value":"db.lan","@language":"en"}, PORT:5432}),
        json!({HOST:"db.lan", PORT:{"@value":"05432","@type":"http://www.w3.org/2001/XMLSchema#int"}}),
        json!({HOST:{"@value":"2026-10-27T00:00:00Z","@type":"http://www.w3.org/2001/XMLSchema#dateTime"}, PORT:5432}),
        json!({HOST:"db.lan", PORT:true}),
    ] {
        assert!(mint(&mut store, &keys).is_err(), "{keys}");
    }
    let ok = mint(
        &mut store,
        &json!({HOST:{"@value":"db.lan","@type":XSD_STRING}, PORT:{"@value":"5432","@type":XSD_INTEGER}}),
    )
    .unwrap();
    // The typed-object and bare spellings are the same terms.
    assert_eq!(
        ok["iri"],
        mint(&mut store, &json!({HOST:"db.lan", PORT:5432})).unwrap()["iri"]
    );
}

#[test]
fn shape_contract_is_enforced() {
    // Key field not declared exactly-one: divergence would union, not conflict.
    let mut loose = store_with(&[ENDPOINT], &shapes("", "2"));
    assert!(mint(&mut loose, &json!({HOST:"h", PORT:1})).is_err());
    // No keyed shape for the type.
    let mut none = store_with(&[ENDPOINT], "@prefix sh: <http://www.w3.org/ns/shacl#> .\n");
    assert!(mint(&mut none, &json!({HOST:"h", PORT:1})).is_err());
    // Two keyed shapes for one type are ambiguous.
    let twice = format!(
        "{}\n<urn:example:Other> <http://www.w3.org/ns/shacl#targetClass> <{ENDPOINT}> ; <{KEY_FIELDS}> ( <{HOST}> ) .\n",
        shapes("", "1")
    );
    assert!(key_spec(&twice, ENDPOINT).is_err());
    // A key field without a declared kind would let 5432 and "5432" diverge.
    let untyped = shapes_kinds("", "1", "", &format!("sh:datatype <{XSD_INTEGER}>"));
    assert!(key_spec(&untyped, ENDPOINT).is_err());
    // A datatype without a canonicalizer refuses the shape.
    let date = shapes_kinds(
        "",
        "1",
        "sh:datatype <http://www.w3.org/2001/XMLSchema#dateTime>",
        &format!("sh:datatype <{XSD_INTEGER}>"),
    );
    assert!(key_spec(&date, ENDPOINT).is_err());
    // An IRI-kind key accepts {"@id"} and refuses a string.
    let iri_host = shapes_kinds(
        "",
        "1",
        "sh:nodeKind sh:IRI",
        &format!("sh:datatype <{XSD_INTEGER}>"),
    );
    let mut iris = store_with(&[ENDPOINT], &iri_host);
    mint(&mut iris, &json!({HOST:{"@id":"urn:host:db"}, PORT:1})).unwrap();
    assert!(mint(&mut iris, &json!({HOST:"urn:host:db", PORT:1})).is_err());
    // Unknown strategy.
    let bad = shapes("quipu:keyStrategy \"uuid\" ;", "1");
    assert!(key_spec(&bad, ENDPOINT).is_err());
}

#[test]
fn namespace_and_request_gates() {
    // A key namespace must select explicit types.
    let mut store = Store::open_in_memory().unwrap();
    let mut cfg = crate::QuipuConfig::default();
    cfg.namespaces.push(Namespace {
        prefix: "ep".into(),
        base_iri: "https://public.example/ep/".into(),
        minting: "key".into(),
        ..Namespace::default()
    });
    assert!(configure(&mut store, &cfg).is_err());
    // keys only in key namespaces, and a key namespace needs keys.
    let mut store = store_with(&[ENDPOINT], &shapes("", "1"));
    assert!(
        tool_entity_ref(
            &mut store,
            &json!({"action":"mint","namespace":"ep","type":ENDPOINT})
        )
        .is_err()
    );
    assert!(
        tool_entity_ref(
            &mut store,
            &json!({"action":"resolve","iri":"https://public.example/ep/x","keys":{}})
        )
        .is_err()
    );
    let mut opaque = Store::open_in_memory().unwrap();
    let mut opaque_cfg = crate::QuipuConfig::default();
    opaque_cfg.namespaces.push(Namespace {
        prefix: "pub".into(),
        base_iri: "https://public.example/id/".into(),
        ..Namespace::default()
    });
    configure(&mut opaque, &opaque_cfg).unwrap();
    apply(&mut opaque).unwrap();
    assert!(
        tool_entity_ref(
            &mut opaque,
            &json!({"action":"mint","namespace":"pub","type":ENDPOINT,"keys":{HOST:"h"}})
        )
        .is_err()
    );
}

#[test]
fn converge_refuses_retracted_or_mutated_identity() {
    let mut store = store_with(&[ENDPOINT], &shapes("", "1"));
    let keys = json!({HOST:"db.lan", PORT:5432});
    let iri = mint(&mut store, &keys).unwrap()["iri"]
        .as_str()
        .unwrap()
        .to_string();
    let id = store.lookup(&iri).unwrap().unwrap();
    store
        .retract_entity(id, None, &crate::time::now_iso(), None)
        .unwrap();
    assert!(
        mint(&mut store, &keys).is_err(),
        "never silently revive a retracted key"
    );
}

#[test]
fn lexical_keys_are_readable_and_need_a_single_type() {
    let lexical = shapes("quipu:keyStrategy \"lexical\" ;", "1");
    let mut store = store_with(&[ENDPOINT], &lexical);
    let out = mint(&mut store, &json!({HOST:"db.lan", PORT:5432})).unwrap();
    assert_eq!(out["iri"], "https://public.example/ep/db.lan+5432");
    let out = mint(&mut store, &json!({HOST:"a b+c/d", PORT:1})).unwrap();
    assert_eq!(out["iri"], "https://public.example/ep/a%20b%2Bc%2Fd+1");
    let mut wide = store_with(&[ENDPOINT, "urn:example:Socket"], &lexical);
    assert!(mint(&mut wide, &json!({HOST:"db.lan", PORT:5432})).is_err());
}

#[test]
fn converge_checks_the_key_values_not_just_the_type() {
    let ttl = shapes("", "1");
    let mut store = store_with(&[ENDPOINT], &ttl);
    let spec = key_spec(&ttl, ENDPOINT).unwrap();
    let keys = json!({HOST:"db.lan", PORT:5432});
    let terms = key_terms(&spec, keys.as_object().unwrap()).unwrap();
    let iri = format!(
        "https://public.example/ep/{}",
        derive_key(&spec, ENDPOINT, &terms)
    );
    // A raw writer puts a live, correctly typed node at the key IRI with the
    // wrong port. Converging onto it would bless a mutated identity.
    let raw = format!(
        "<{iri}> <{}> <{ENDPOINT}> .\n<{iri}> <{HOST}> \"db.lan\" .\n<{iri}> <{PORT}> \"9999\"^^<{XSD_INTEGER}> .\n",
        crate::namespace::RDF_TYPE
    );
    crate::rdf::ingest_rdf_to_graph(
        &mut store,
        raw.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        &crate::time::now_iso(),
        None,
        Some("raw"),
        0,
    )
    .unwrap();
    assert!(
        mint(&mut store, &keys).is_err(),
        "a key IRI whose key values differ must refuse, not converge"
    );
}
