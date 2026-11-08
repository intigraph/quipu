//! CLI namespace selection is independent from the host's ambient namespace.
#![cfg(feature = "shacl")]
use quipu::{Store, rdf::ingest_rdf, types::Value};
use std::{fs, process::Command};

#[test]
fn reason_uses_the_configured_store_namespace_for_bare_rules() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("store.db");
    let ns = "https://example.org/fixture#";
    fs::create_dir(dir.path().join(".bobbin")).unwrap();
    fs::write(
        dir.path().join(".bobbin/config.toml"),
        format!("[quipu]\nbase_ns = {ns:?}\n"),
    )
    .unwrap();
    let mut store = Store::open(db.to_str().unwrap()).unwrap();
    store.set_base_ns(ns);
    ingest_rdf(
        &mut store,
        format!("<urn:subject> <{ns}input> <urn:object> .").as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    drop(store);
    fs::write(
        dir.path().join("rules.ttl"),
        r#"
@prefix rule: <http://quipu.local/rule#> .
<urn:rule> a rule:Rule; rule:id "custom";
 rule:head "output(?x, ?y)"; rule:body "input(?x, ?y)" .
"#,
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(dir.path())
        .args(["reason", "--rules", "rules.ttl", "--db"])
        .arg(&db)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let store = Store::open(db.to_str().unwrap()).unwrap();
    let predicate = store
        .lookup(&format!("{ns}output"))
        .unwrap()
        .expect("configured output predicate");
    let object = store.lookup("urn:object").unwrap().unwrap();
    let subject = store.lookup("urn:subject").unwrap().unwrap();
    let graph = store
        .lookup("urn:quipu:graph:root#inferred")
        .unwrap()
        .unwrap();
    let facts = store.current_facts_in_graph(graph).unwrap();
    assert!(
        facts.iter().any(|f| f.entity == subject
            && f.attribute == predicate
            && f.value == Value::Ref(object))
    );
    assert!(
        store
            .lookup(&format!("{}output", quipu::namespace::DEFAULT_BASE_NS))
            .unwrap()
            .is_none()
    );
}
