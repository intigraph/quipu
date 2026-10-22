//! A graph IRI held by both the local store and an attached layer stays composed
//! after a `GRAPH ?g` query (aegis-gwu).
//!
//! `GRAPH ?g` ranges the layer's graphs, so it resolves the layer's graph ids.
//! Resolving used to memoize IRI -> id in both directions, which recorded the
//! LAYER id as what the local `lookup` returns for that IRI. `lookup_all` then
//! built its id set from the layer id alone, and a later `GRAPH <iri>` silently
//! dropped the local copy of the same graph.
use quipu::Store;
use quipu::sparql::QueryResult;
use quipu::store::attach::Attachment;

const PREFIX: &str = "PREFIX ex: <http://example.org/> ";
const SHARED: &str = "SELECT ?id WHERE { GRAPH ex:shared { ex:s ex:id ?id } } ORDER BY ?id";
const ANY_GRAPH: &str = "SELECT ?id WHERE { GRAPH ?g { ex:a ex:id ?id } } ORDER BY ?id";

fn seed(store: &mut Store, graph: i64, turtle: &str) {
    let turtle = format!("@prefix ex: <http://example.org/> . {turtle}");
    quipu::rdf::ingest_rdf_to_graph(
        store,
        turtle.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-10-08T00:00:00Z",
        None,
        None,
        graph,
    )
    .unwrap();
}

fn ids(store: &Store, query: &str) -> Vec<String> {
    match quipu::sparql::query(store, &format!("{PREFIX}{query}")).unwrap() {
        QueryResult::Select { rows, .. } => rows
            .iter()
            .map(|row| match &row["id"] {
                quipu::types::Value::Str(s) => s.clone(),
                other => panic!("unexpected binding {other:?}"),
            })
            .collect(),
        other => panic!("expected a SELECT result, got {other:?}"),
    }
}

/// Main holds ex:board and ex:shared; the layer (respaced to space 4) holds
/// ex:frozen and its own copy of ex:shared.
fn composed(dir: &std::path::Path) -> Store {
    let (main, raw, layer) = (
        dir.join("main.db"),
        dir.join("raw.db"),
        dir.join("layer.db"),
    );
    {
        let mut s = Store::open(main.to_str().unwrap()).unwrap();
        let board = s.graph_create("http://example.org/board").unwrap();
        seed(&mut s, board, r#"ex:a ex:id "a" ."#);
        let shared = s.graph_create("http://example.org/shared").unwrap();
        seed(&mut s, shared, r#"ex:s ex:id "main-copy" ."#);
    }
    {
        let mut s = Store::open(raw.to_str().unwrap()).unwrap();
        let frozen = s.graph_create("http://example.org/frozen").unwrap();
        seed(&mut s, frozen, r#"ex:a ex:id "frozen-a" ."#);
        let shared = s.graph_create("http://example.org/shared").unwrap();
        seed(&mut s, shared, r#"ex:s ex:id "layer-copy" ."#);
    }
    quipu::store::respace::respace_file(&raw, &layer, 4).unwrap();
    Store::open_with_attachments(
        main.to_str().unwrap(),
        &[Attachment::read_only("layer", layer.to_str().unwrap())],
    )
    .unwrap()
}

#[test]
fn a_shared_graph_keeps_its_local_copy_after_a_graph_variable_query() {
    let dir = tempfile::tempdir().unwrap();
    let store = composed(dir.path());
    let both = vec!["layer-copy".to_string(), "main-copy".to_string()];
    assert_eq!(ids(&store, SHARED), both, "fresh store");
    let local = store.lookup("http://example.org/shared").unwrap();
    assert_eq!(
        ids(&store, ANY_GRAPH),
        vec!["a".to_string(), "frozen-a".to_string()]
    );
    assert_eq!(
        store.lookup("http://example.org/shared").unwrap(),
        local,
        "resolving the layer's graph ids must not change what the local lookup returns"
    );
    assert_eq!(
        ids(&store, SHARED),
        both,
        "after GRAPH ?g the local copy must remain"
    );
}

#[test]
fn the_order_does_not_matter() {
    // GRAPH ?g first, on a fresh store: the shared graph is still composed.
    let dir = tempfile::tempdir().unwrap();
    let store = composed(dir.path());
    ids(&store, ANY_GRAPH);
    assert_eq!(
        ids(&store, SHARED),
        vec!["layer-copy".to_string(), "main-copy".to_string()]
    );
}
