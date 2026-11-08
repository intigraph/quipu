//! Active named-graph scope must survive subquery projection.
use quipu::{Store, Value};

const PREFIX: &str = "PREFIX ex: <http://example.org/> ";

fn fixture() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    // E is a registered but empty named graph, matching the reported leak.
    for (iri, turtle) in [
        ("http://example.org/A", "ex:a1 ex:p 1 . ex:a2 ex:p 2 ."),
        ("http://example.org/E", ""),
        ("http://example.org/B", "ex:b ex:p 3 ."),
    ] {
        let graph = store.graph_create(iri).unwrap();
        if !turtle.is_empty() {
            quipu::rdf::ingest_rdf_to_graph(
                &mut store,
                format!("@prefix ex: <http://example.org/> . {turtle}").as_bytes(),
                oxrdfio::RdfFormat::Turtle,
                None,
                "2026-11-06T00:00:00Z",
                None,
                None,
                graph,
            )
            .unwrap();
        }
    }
    // Default graph facts must not become named-graph subquery results either.
    quipu::rdf::ingest_rdf_to_graph(
        &mut store,
        b"<http://example.org/root> <http://example.org/p> 9 .".as_slice(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-11-06T00:00:00Z",
        None,
        None,
        0,
    )
    .unwrap();
    store
}

fn counts(store: &Store, body: &str) -> Vec<(String, i64)> {
    let result = quipu::sparql::query(store, &format!("{PREFIX}{body}")).unwrap();
    let mut rows: Vec<_> = result
        .rows()
        .iter()
        .map(|row| {
            let Value::Ref(graph) = row["g"] else {
                panic!("expected graph IRI: {row:?}");
            };
            let Value::Int(n) = row["n"] else {
                panic!("expected integer count: {row:?}");
            };
            (store.resolve(graph).unwrap(), n)
        })
        .collect();
    rows.sort();
    rows
}

#[test]
fn values_bound_graph_subquery_cannot_borrow_other_graph_rows() {
    let store = fixture();
    let query = "SELECT ?g (COUNT(*) AS ?n) WHERE {
        VALUES ?g { ex:A ex:E }
        GRAPH ?g { { SELECT ?s ?v WHERE { ?s ex:p ?v FILTER(?v >= 1 && ?v < 3) } } }
    } GROUP BY ?g";
    assert_eq!(
        counts(&store, query),
        vec![("http://example.org/A".into(), 2)]
    );
}

#[test]
fn nonempty_named_graphs_keep_distinct_subquery_counts() {
    let store = fixture();
    let query = "SELECT ?g (COUNT(*) AS ?n) WHERE {
        VALUES ?g { ex:A ex:B }
        GRAPH ?g { { SELECT ?s ?v WHERE { ?s ex:p ?v } } }
    } GROUP BY ?g";
    assert_eq!(
        counts(&store, query),
        vec![
            ("http://example.org/A".into(), 2),
            ("http://example.org/B".into(), 1)
        ]
    );
}

#[test]
fn plain_variable_graph_and_constant_empty_graph_are_scope_controls() {
    let store = fixture();
    assert_eq!(
        counts(
            &store,
            "SELECT ?g (COUNT(*) AS ?n) WHERE {
            VALUES ?g { ex:A ex:E } GRAPH ?g { ?s ex:p ?v FILTER(?v >= 1 && ?v < 3) }
        } GROUP BY ?g"
        ),
        vec![("http://example.org/A".into(), 2)]
    );
    let empty = quipu::sparql::query(
        &store,
        &format!(
            "{PREFIX}
        SELECT ?s WHERE {{ GRAPH ex:E {{ {{ SELECT ?s WHERE {{ ?s ex:p ?v }} }} }} }}"
        ),
    )
    .unwrap();
    assert!(
        empty.rows().is_empty(),
        "empty constant graph leaked: {empty:?}"
    );
}

#[test]
fn reported_temporal_subquery_stays_inside_each_values_graph() {
    let mut store = fixture();
    let graph = store.lookup("http://example.org/A").unwrap().unwrap();
    quipu::rdf::ingest_rdf_to_graph(
        &mut store,
        br#"@prefix ex: <http://example.org/> .
            @prefix schema: <https://schema.org/> .
            @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
            ex:done schema:endTime "2026-10-29T02:00:00Z"^^xsd:dateTime ."#
            .as_slice(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-11-06T00:00:00Z",
        None,
        None,
        graph,
    )
    .unwrap();
    assert_eq!(
        counts(
            &store,
            r#"
        PREFIX schema: <https://schema.org/>
        PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
        SELECT ?g (COUNT(*) AS ?n) WHERE {
            VALUES ?g { ex:A ex:E }
            GRAPH ?g { { SELECT ?e ?t WHERE {
                ?e schema:endTime ?t .
                FILTER(?t >= xsd:dateTime("2026-10-29T01:30:20Z") &&
                       ?t < xsd:dateTime("2026-10-30T01:30:20Z"))
            } } }
        } GROUP BY ?g"#
        ),
        vec![("http://example.org/A".into(), 1)]
    );
}

#[test]
fn graph_variable_is_bound_even_when_inner_select_does_not_project_it() {
    let store = fixture();
    let result = quipu::sparql::query(
        &store,
        &format!(
            "{PREFIX}SELECT ?g ?s WHERE {{ GRAPH ?g {{
            {{ SELECT ?s WHERE {{ ?s ex:p ?v }} }}
        }} }}"
        ),
    )
    .unwrap();
    assert_eq!(result.rows().len(), 3);
    for row in result.rows() {
        let Value::Ref(graph) = row["g"] else {
            panic!("GRAPH binding lost at projection boundary: {row:?}");
        };
        let Value::Ref(subject) = row["s"] else {
            panic!("expected subject IRI");
        };
        let graph = store.resolve(graph).unwrap();
        let subject = store.resolve(subject).unwrap();
        assert_eq!(
            graph,
            if subject.ends_with("/b") {
                "http://example.org/B"
            } else {
                "http://example.org/A"
            }
        );
    }
}

#[test]
fn a_subquery_local_graph_variable_stays_local_when_not_projected() {
    let store = fixture();
    let query = "SELECT ?g (COUNT(*) AS ?n) WHERE {
        VALUES ?g { ex:A ex:B }
        GRAPH ?g { { SELECT ?s WHERE { VALUES ?g { ex:Other } ?s ex:p ?v } } }
    } GROUP BY ?g";
    assert_eq!(
        counts(&store, query),
        vec![
            ("http://example.org/A".into(), 2),
            ("http://example.org/B".into(), 1)
        ]
    );
}

#[test]
fn a_projected_conflicting_graph_binding_is_not_overwritten() {
    let store = fixture();
    let result = quipu::sparql::query(
        &store,
        &format!(
            "{PREFIX}SELECT ?g WHERE {{ GRAPH ?g {{
            {{ SELECT (ex:Other AS ?g) WHERE {{ ?s ex:p ?v }} }}
        }} }}"
        ),
    )
    .unwrap();
    assert!(
        result.rows().is_empty(),
        "incompatible GRAPH name survived: {result:?}"
    );
}
