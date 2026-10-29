//! aegis-gxfm: `quipu retract` runs the deletion gate.
#![cfg(feature = "shacl")]

use std::process::Command;

use quipu::{Datum, Op, Store, Value};

const TS: &str = "2026-10-28T00:00:00Z";
const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <urn:t:> .
ex:PersonShape a sh:NodeShape ;
    sh:targetClass ex:Person ;
    sh:property [ sh:path ex:name ; sh:minCount 1 ] .
"#;

fn seed(path: &str) {
    let mut store = Store::open(path).unwrap();
    let carol = store.intern("urn:t:carol").unwrap();
    let rdf_type = store
        .intern("http://www.w3.org/1999/02/22-rdf-syntax-ns#type")
        .unwrap();
    let person = store.intern("urn:t:Person").unwrap();
    let name = store.intern("urn:t:name").unwrap();
    let d = |a, v| Datum {
        entity: carol,
        attribute: a,
        value: v,
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    store
        .transact(
            &[
                d(rdf_type, Value::Ref(person)),
                d(name, Value::Str("Carol".into())),
            ],
            TS,
            None,
            Some("seed"),
        )
        .unwrap();
    store.load_shapes("people", SHAPES, TS).unwrap();
}

#[test]
fn cli_retract_of_a_required_value_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("q.db");
    let path = db.to_str().unwrap();
    seed(path);
    let out = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args([
            "retract",
            "urn:t:carol",
            "--predicate",
            "urn:t:name",
            "--db",
            path,
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains("introduced by a deletion"), "{stderr}");
    let store = Store::open(path).unwrap();
    let carol = store.lookup("urn:t:carol").unwrap().unwrap();
    assert_eq!(
        store.entity_facts(carol).unwrap().len(),
        2,
        "nothing written"
    );
}
