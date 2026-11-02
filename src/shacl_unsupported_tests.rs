use super::*;

const PREFIXES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
                        @prefix ex: <http://example.org/> .\n\
                        @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n";

/// A node shape with a SPARQL-based constraint that a negative age violates.
const NODE_SPARQL: &str = "ex:PersonShape a sh:NodeShape ; sh:targetClass ex:Person ;\n\
    sh:sparql [ a sh:SPARQLConstraint ;\n\
        sh:select \"SELECT $this WHERE { $this ex:age ?a . FILTER (?a < 0) }\" ] .\n";

/// The Core control: the same class, an age that must be an integer.
const CORE: &str = "ex:PersonShape a sh:NodeShape ; sh:targetClass ex:Person ;\n\
    sh:property [ sh:path ex:age ; sh:datatype xsd:integer ] .\n";

fn shapes(body: &str) -> String {
    format!("{PREFIXES}{body}")
}

fn ex(local: &str) -> String {
    format!("http://example.org/{local}")
}

/// The finding names `shape`, as the node itself or the named shape above it.
fn names(found: &Unsupported, shape: &str) -> bool {
    found.node == ex(shape) || found.shape.as_deref() == Some(ex(shape).as_str())
}

#[test]
fn a_node_shape_sparql_constraint_is_found() {
    let found = find_unsupported(&shapes(NODE_SPARQL))
        .unwrap()
        .expect("found");
    assert!(names(&found, "PersonShape"), "{found:?}");
    assert!(
        ["sh:sparql", "sh:SPARQLConstraint", "sh:select"].contains(&found.feature.as_str()),
        "{found:?}"
    );
}

#[test]
fn a_property_shape_sparql_constraint_names_its_shape() {
    let body = "ex:S a sh:NodeShape ; sh:targetClass ex:Person ;\n\
        sh:property [ sh:path ex:age ; sh:sparql [ sh:select \"SELECT $this WHERE { }\" ] ] .\n";
    let found = find_unsupported(&shapes(body)).unwrap().expect("found");
    assert!(found.node.starts_with("_:"), "{found:?}");
    assert_eq!(found.shape, Some(ex("S")), "{found:?}");
}

#[test]
fn a_sparql_constraint_reached_through_sh_or_is_found() {
    let body = "ex:Outer a sh:NodeShape ; sh:targetClass ex:Person ; sh:or ( ex:Inner ) .\n\
        ex:Inner a sh:NodeShape ; sh:sparql [ sh:select \"SELECT $this WHERE { }\" ] .\n";
    let found = find_unsupported(&shapes(body)).unwrap().expect("found");
    assert!(names(&found, "Inner"), "{found:?}");
}

#[test]
fn a_sparql_based_constraint_component_is_found() {
    let body = "ex:PositiveComponent a sh:ConstraintComponent ;\n\
        sh:parameter [ sh:path ex:positive ] ; sh:validator ex:PositiveValidator .\n\
        ex:PositiveValidator a sh:SPARQLAskValidator ; sh:ask \"ASK { FILTER ($value > 0) }\" .\n";
    let found = find_unsupported(&shapes(body)).unwrap().expect("found");
    assert!(
        names(&found, "PositiveComponent") || names(&found, "PositiveValidator"),
        "{found:?}"
    );
}

#[test]
fn core_shapes_and_prefix_declarations_pass() {
    let body = format!(
        "{CORE}ex: sh:declare [ sh:prefix \"ex\" ; sh:namespace \"http://example.org/\"^^xsd:anyURI ] .\n\
         ex:PersonShape sh:prefixes ex: .\n"
    );
    assert_eq!(find_unsupported(&shapes(&body)).unwrap(), None);
    refuse_unsupported_shapes(&shapes(&body)).unwrap();
}

#[test]
fn the_refusal_names_the_shape_and_the_feature() {
    let err = refuse_unsupported_shapes(&shapes(NODE_SPARQL))
        .unwrap_err()
        .to_string();
    assert!(err.contains(REFUSAL), "{err}");
    assert!(err.contains(&ex("PersonShape")), "{err}");
    assert!(err.contains("sh:"), "{err}");
}

#[test]
fn unparseable_shapes_are_an_error_not_a_pass() {
    assert!(find_unsupported("ex:S a sh:NodeShape").is_err());
}

#[cfg(feature = "shacl")]
mod enforcement {
    use super::*;
    use crate::store::Store;
    use oxrdfio::RdfFormat;

    const T: &str = "2026-11-01T00:00:00Z";
    const VIOLATING: &str =
        "@prefix ex: <http://example.org/> .\nex:ada a ex:Person ; ex:age -1 .\n";

    #[test]
    fn per_request_shapes_with_sparql_are_refused() {
        let err = crate::shacl::Validator::from_turtle(&shapes(NODE_SPARQL))
            .err()
            .expect("refused")
            .to_string();
        assert!(err.contains(REFUSAL), "{err}");
    }

    #[test]
    fn core_constraints_are_still_enforced() {
        let validator = crate::shacl::Validator::from_turtle(&shapes(CORE)).unwrap();
        let bad = "@prefix ex: <http://example.org/> .\nex:ada a ex:Person ; ex:age \"old\" .\n";
        assert!(!validator.validate(bad.as_bytes()).unwrap().conforms);
        assert!(validator.validate(VIOLATING.as_bytes()).unwrap().conforms);
    }

    #[test]
    fn loading_a_sparql_shape_is_refused_and_stores_nothing() {
        // A `&mut` binding: `load_shapes` takes `&mut self` once C1 lands.
        let store = &mut Store::open_in_memory().unwrap();
        let head = store.latest_tx_id().unwrap();
        let err = store
            .load_shapes("people", &shapes(NODE_SPARQL), T)
            .unwrap_err()
            .to_string();
        assert!(err.contains(REFUSAL), "{err}");
        assert!(store.list_shapes().unwrap().is_empty());
        assert_eq!(store.latest_tx_id().unwrap(), head);
        store.load_shapes("people", &shapes(CORE), T).unwrap();
    }

    #[test]
    fn a_stored_sparql_shape_fails_writes_closed_until_removed() {
        // A store written before this guard can already hold such a set.
        let store = &mut Store::open_in_memory().unwrap();
        store
            .conn
            .execute(
                "INSERT INTO shapes (name, turtle, loaded_at, valid_from, valid_to, tx) \
                 VALUES (?1, ?2, ?3, ?3, NULL, NULL)",
                rusqlite::params!["legacy", shapes(NODE_SPARQL), T],
            )
            .unwrap();
        let head = store.latest_tx_id().unwrap();
        let write = |store: &mut Store| {
            crate::rdf::ingest_rdf_to_graph(
                store,
                VIOLATING.as_bytes(),
                RdfFormat::Turtle,
                None,
                T,
                None,
                None,
                0,
            )
        };
        let err = write(store).unwrap_err().to_string();
        assert!(err.contains(REFUSAL), "the write names why: {err}");
        assert_eq!(store.latest_tx_id().unwrap(), head, "nothing was written");
        assert!(store.current_facts_in_graph(0).unwrap().is_empty());

        // The operator remedy: remove the set; writes are then judged by Core.
        assert!(store.remove_shapes("legacy").unwrap());
        write(store).unwrap();
    }
}
