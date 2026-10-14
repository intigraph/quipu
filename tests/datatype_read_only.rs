//! Literal datatype projections must not require a writable term dictionary.

use quipu::{Store, Value};

const XSD: &str = "http://www.w3.org/2001/XMLSchema#";

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.db");
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    quipu::ingest_rdf(
        &mut store,
        &b"<urn:item> <urn:due> \"2026-10-14\"^^<http://www.w3.org/2001/XMLSchema#date> .\n"[..],
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-10-14T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    assert_eq!(store.lookup(&format!("{XSD}date")).unwrap(), None);
    drop(store);
    (root, path)
}

#[test]
fn stored_date_projects_its_datatype_on_a_read_only_connection() {
    let (_root, path) = fixture();
    let before = std::fs::read(&path).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let store = Store::open_read_only(path.to_str().unwrap()).unwrap();
    let result = quipu::sparql::query(
        &store,
        r#"SELECT (STR(?v) AS ?value)
                  (IF(isIRI(?v), "iri", STR(DATATYPE(?v))) AS ?kind)
           WHERE { <urn:item> <urn:due> ?v }"#,
    )
    .unwrap();
    assert_eq!(result.rows().len(), 1);
    let row = &result.rows()[0];
    assert_eq!(row.get("value"), Some(&Value::Str("2026-10-14".into())));
    assert_eq!(row.get("kind"), Some(&Value::Str(format!("{XSD}date"))));
    let wire = quipu::tool_query(
        &store,
        &serde_json::json!({
            "query": "SELECT (STR(DATATYPE(?v)) AS ?kind) WHERE { <urn:item> <urn:due> ?v }",
            "verbose": true
        }),
    )
    .unwrap();
    assert_eq!(wire["rows"][0]["kind"], format!("{XSD}date"));
    assert_eq!(store.lookup(&format!("{XSD}date")).unwrap(), None);
    drop(store);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
}

#[test]
fn datatype_iris_keep_identity_and_invalid_operands_remain_unbound() {
    let (_root, path) = fixture();
    let store = Store::open_read_only(path.to_str().unwrap()).unwrap();
    let result = quipu::sparql::query(
        &store,
        r#"PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
           SELECT ?datatype ?same ?iri ?invalid ?missing WHERE {
             <urn:item> <urn:due> ?v
             BIND(DATATYPE(?v) AS ?datatype)
             BIND(sameTerm(?datatype, xsd:date) AS ?same)
             BIND(isIRI(?datatype) AS ?iri)
             BIND(DATATYPE(<urn:item>) AS ?invalid)
             BIND(DATATYPE(?unbound) AS ?missing)
             FILTER(?datatype = xsd:date)
           }"#,
    )
    .unwrap();
    assert_eq!(result.rows().len(), 1);
    let row = &result.rows()[0];
    let Value::Ref(id) = row["datatype"] else {
        panic!("DATATYPE must return an IRI, not a string");
    };
    assert_eq!(store.resolve(id).unwrap(), format!("{XSD}date"));
    assert_eq!(row.get("same"), Some(&Value::Bool(true)));
    assert_eq!(row.get("iri"), Some(&Value::Bool(true)));
    assert!(!row.contains_key("invalid"));
    assert!(!row.contains_key("missing"));
    assert_eq!(store.lookup(&format!("{XSD}date")).unwrap(), None);
}

#[test]
fn read_only_datatype_preserves_arbitrary_literal_types() {
    let (_root, path) = fixture();
    let store = Store::open_read_only(path.to_str().unwrap()).unwrap();
    for (literal, datatype) in [
        (r#""2026-10-14"^^xsd:date"#, format!("{XSD}date")),
        (
            r#""2026-10-14T00:00:00+02:00"^^xsd:dateTime"#,
            format!("{XSD}dateTime"),
        ),
        (r#""1.20"^^xsd:decimal"#, format!("{XSD}decimal")),
        (r#""opaque"^^<urn:custom-type>"#, "urn:custom-type".into()),
        (r#""plain""#, format!("{XSD}string")),
        ("42", format!("{XSD}integer")),
        ("true", format!("{XSD}boolean")),
        (
            r#""bonjour"@fr"#,
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString".into(),
        ),
    ] {
        let result = quipu::sparql::query(
            &store,
            &format!("PREFIX xsd: <{XSD}> SELECT (STR(DATATYPE({literal})) AS ?kind) WHERE {{}}"),
        )
        .unwrap();
        assert_eq!(result.rows()[0].get("kind"), Some(&Value::Str(datatype)));
    }
}

#[test]
fn datatype_reuses_an_existing_dictionary_id() {
    let (_root, path) = fixture();
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let date = store.intern(&format!("{XSD}date")).unwrap();
    drop(store);
    let store = Store::open_read_only(path.to_str().unwrap()).unwrap();
    let result = quipu::sparql::query(
        &store,
        "SELECT (DATATYPE(?v) AS ?kind) WHERE { <urn:item> <urn:due> ?v }",
    )
    .unwrap();
    assert_eq!(result.rows()[0].get("kind"), Some(&Value::Ref(date)));
}

#[test]
fn transient_type_admission_is_bounded_without_evicting_existing_results() {
    let (_root, path) = fixture();
    let store = Store::open_read_only(path.to_str().unwrap()).unwrap();
    store.set_term_cache_limit(1);
    for _ in 0..2 {
        let result = quipu::sparql::query(
            &store,
            "SELECT (STR(DATATYPE(?v)) AS ?kind) WHERE { <urn:item> <urn:due> ?v }",
        )
        .unwrap();
        assert_eq!(
            result.rows()[0].get("kind"),
            Some(&Value::Str(format!("{XSD}date")))
        );
    }
    let refused = quipu::sparql::query(
        &store,
        r#"SELECT (DATATYPE("opaque"^^<urn:new-type>) AS ?kind) WHERE {}"#,
    )
    .unwrap();
    assert!(!refused.rows()[0].contains_key("kind"));
    assert_eq!(store.lookup("urn:new-type").unwrap(), None);
}
