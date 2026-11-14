//! Retiring a client protocol must not retire its ordinary graph data.
#![cfg(feature = "shacl")]

#[test]
fn former_file_records_remain_plain_queryable_facts_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("retained.db");
    let rdf = br#"<urn:quipu:memory:revision:retained> a <https://example.org/Artifact> ;
        <https://quipu.dev/memory#kind> "revision" ;
        <http://www.w3.org/1999/02/22-rdf-syntax-ns#value> "{\"text\":\"retained source\"}" ."#;
    {
        let mut store = quipu::Store::open(path.to_str().unwrap()).unwrap();
        quipu::ingest_rdf(
            &mut store,
            rdf.as_slice(),
            oxrdfio::RdfFormat::Turtle,
            None,
            "2026-01-01T00:00:00Z",
            None,
            None,
        )
        .unwrap();
    }
    let store = quipu::Store::open(path.to_str().unwrap()).unwrap();
    let result = quipu::sparql::query(&store,
        "SELECT ?text WHERE { <urn:quipu:memory:revision:retained> <http://www.w3.org/1999/02/22-rdf-syntax-ns#value> ?text }").unwrap();
    assert_eq!(result.rows().len(), 1);
    assert_eq!(
        result.rows()[0].get("text"),
        Some(&quipu::Value::Str("{\"text\":\"retained source\"}".into()))
    );
    let graph = quipu::sparql::query(&store,
        "CONSTRUCT { <urn:quipu:memory:revision:retained> ?p ?o } WHERE { <urn:quipu:memory:revision:retained> ?p ?o }").unwrap();
    match graph {
        quipu::sparql::QueryResult::Graph(triples) => assert_eq!(triples.len(), 3),
        _ => panic!("ordinary graph export must preserve all three facts"),
    }
}

#[test]
fn retired_memory_command_is_refused_before_creating_a_store() {
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path().join("config"))
        .env("XDG_DATA_HOME", dir.path().join("data"))
        .args(["memory", "read", "absent.json", "--db", "must-not-exist.db"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown command: memory"));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
