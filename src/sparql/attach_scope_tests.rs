//! A query that reads no attached graph answers as the detached store does,
//! and one that may read an attached graph still sees it (aegis-xacr).
use super::super::{QueryResult, TemporalContext, query_temporal};
use crate::Store;
use crate::store::attach::Attachment;
use oxrdfio::RdfFormat;
use spargebra::Query;

const TS: &str = "2026-10-08T00:00:00Z";
const PREFIX: &str = "PREFIX ex: <http://example.org/> ";

fn seed(store: &mut Store, graph: i64, triples: &str) {
    let turtle = format!("@prefix ex: <http://example.org/> . {triples}");
    crate::rdf::ingest_rdf_to_graph(
        store,
        turtle.as_bytes(),
        RdfFormat::Turtle,
        None,
        TS,
        None,
        None,
        graph,
    )
    .unwrap();
}

/// `main` holds a local board; the layer holds a frozen graph and a second
/// copy of `ex:shared`, so the shared IRI has an id in each term space.
struct Fixture {
    _dir: tempfile::TempDir,
    main: String,
    layer: String,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.db");
    let raw = dir.path().join("raw.db");
    let layer = dir.path().join("layer.db");
    {
        let mut store = Store::open(main.to_str().unwrap()).unwrap();
        let board = store.graph_create("http://example.org/board").unwrap();
        seed(
            &mut store,
            board,
            r#"ex:a a ex:Issue ; ex:id "a" ; ex:status "open" ; ex:end "2026-10-02" .
               ex:b a ex:Issue ; ex:id "b" ; ex:status "done" ; ex:end "2026-10-09" .
               ex:c a ex:Issue ; ex:id "c" ; ex:status "done" ."#,
        );
        let shared = store.graph_create("http://example.org/shared").unwrap();
        seed(&mut store, shared, r#"ex:s ex:id "main-copy" ."#);
    }
    {
        let mut store = Store::open(raw.to_str().unwrap()).unwrap();
        let frozen = store.graph_create("http://example.org/frozen").unwrap();
        seed(
            &mut store,
            frozen,
            r#"ex:a ex:id "frozen-a" . ex:z a ex:Issue ; ex:id "z" ; ex:status "open" ."#,
        );
        let shared = store.graph_create("http://example.org/shared").unwrap();
        seed(&mut store, shared, r#"ex:s ex:id "layer-copy" ."#);
    }
    crate::store::respace::respace_file(&raw, &layer, 4).unwrap();
    Fixture {
        main: main.to_str().unwrap().to_string(),
        layer: layer.to_str().unwrap().to_string(),
        _dir: dir,
    }
}

fn attached(f: &Fixture) -> Store {
    let store =
        Store::open_with_attachments(&f.main, &[Attachment::read_only("layer", &f.layer)]).unwrap();
    assert!(store.has_attachments());
    store
}

fn rows(store: &Store, query: &str) -> Vec<String> {
    render(
        query_temporal(
            store,
            &format!("{PREFIX}{query}"),
            &TemporalContext::default(),
        )
        .unwrap(),
    )
}

/// Evaluate under exactly the view the caller set, skipping the scope decision
/// `query_temporal` makes. Only the controls use this.
fn rows_in_view(store: &Store, query: &str, composes: bool) -> Vec<String> {
    let parsed = super::super::sparql_parser()
        .parse_query(&format!("{PREFIX}{query}"))
        .unwrap();
    let _view = store.scope_composition(composes);
    render(super::super::eval_parsed(store, parsed, &TemporalContext::default()).unwrap())
}

fn render(result: QueryResult) -> Vec<String> {
    match result {
        QueryResult::Select { rows, .. } => {
            let mut rows: Vec<String> = rows
                .iter()
                .map(|row| {
                    let mut cells: Vec<String> =
                        row.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
                    cells.sort();
                    cells.join(" ")
                })
                .collect();
            rows.sort();
            rows
        }
        other => panic!("not a SELECT: {other:?}"),
    }
}

fn composes(store: &Store, query: &str) -> bool {
    let parsed = super::super::sparql_parser()
        .parse_query(&format!("{PREFIX}{query}"))
        .unwrap();
    let dataset = match &parsed {
        Query::Select { dataset, .. }
        | Query::Construct { dataset, .. }
        | Query::Describe { dataset, .. }
        | Query::Ask { dataset, .. } => dataset.as_ref(),
    };
    let ctx = TemporalContext::default();
    let scoped = super::super::apply_dataset(store, dataset, &ctx).unwrap();
    super::composes(store, &parsed, &scoped.graph, &ctx).unwrap()
}

/// The shapes whose fast paths were gated on attachments: COUNT, OPTIONAL,
/// FILTER, GROUP BY, ORDER BY with LIMIT, and a two-pattern join.
const LOCAL: &[&str] = &[
    "SELECT ?s ?id WHERE { GRAPH ex:board { ?s a ex:Issue ; ex:id ?id } }",
    "SELECT (COUNT(*) AS ?n) WHERE { GRAPH ex:board { ?s ?p ?o } }",
    "SELECT (COUNT(DISTINCT ?s) AS ?n) WHERE { GRAPH ex:board { ?s a ex:Issue } }",
    "SELECT ?s ?end WHERE { GRAPH ex:board { ?s ex:status \"done\" OPTIONAL { ?s ex:end ?end } } }",
    "SELECT ?s WHERE { GRAPH ex:board { ?s ex:end ?e FILTER(?e >= \"2026-10-05\") } }",
    "SELECT ?st (COUNT(*) AS ?n) WHERE { GRAPH ex:board { ?s ex:status ?st } } GROUP BY ?st",
    "SELECT ?s WHERE { GRAPH ex:board { ?s ex:id ?id } } ORDER BY DESC(?id) LIMIT 2",
    "SELECT ?s WHERE { ?s ?p ?o }",
];

#[test]
fn a_query_that_reads_no_attached_graph_answers_as_the_detached_store() {
    let f = fixture();
    let store = attached(&f);
    let detached = Store::open(&f.main).unwrap();
    assert!(!detached.has_attachments());
    for query in LOCAL {
        assert!(!composes(&store, query), "{query}");
        let answer = rows(&store, query);
        assert_eq!(answer, rows(&detached, query), "{query}");
        // The composed view gives the same answer, so suspending it lost nothing.
        assert_eq!(answer, rows_in_view(&store, query, true), "{query}");
    }
    assert!(store.has_attachments(), "the composed view is restored");
}

#[test]
fn a_query_that_may_read_an_attached_graph_still_sees_it() {
    for (query, expect) in [
        ("SELECT ?id WHERE { GRAPH ex:frozen { ?s ex:id ?id } }", 2),
        ("SELECT ?id FROM ex:frozen WHERE { ?s ex:id ?id }", 2),
        ("SELECT ?id WHERE { GRAPH ?g { ex:a ex:id ?id } }", 2),
        ("SELECT ?id WHERE { GRAPH ex:shared { ex:s ex:id ?id } }", 2),
        (
            "SELECT ?s WHERE { GRAPH ex:board { ?s a ex:Issue } \
             FILTER EXISTS { GRAPH ex:frozen { ?s ex:id ?x } } }",
            1,
        ),
    ] {
        // A fresh store per query: on base main, a GRAPH ?g query leaves state
        // that makes a later query on an IRI both files hold drop the local
        // copy (filed separately). These cases must not depend on that.
        let f = fixture();
        let store = attached(&f);
        assert!(composes(&store, query), "{query}");
        let answer = rows(&store, query);
        assert_eq!(answer.len(), expect, "{query}: {answer:?}");
        // The control: hiding the attachment loses those rows, so the counts
        // above can only hold if the query kept the composed view.
        assert_ne!(answer, rows_in_view(&store, query, false), "{query}");
    }
}

#[test]
fn the_view_is_restored_however_queries_nest() {
    let f = fixture();
    let store = attached(&f);
    {
        let _outer = store.scope_composition(false);
        assert!(!store.has_attachments());
        // Only the composed SQL and the gate change; dictionaries stay mounted.
        assert!(!store.attachments().is_empty());
        assert_eq!(store.facts_source(), "facts");
        // A composing query inside a suspended one must still see the layer.
        assert_eq!(
            rows(
                &store,
                "SELECT ?id WHERE { GRAPH ex:frozen { ?s ex:id ?id } }"
            )
            .len(),
            2
        );
        assert!(!store.has_attachments(), "the outer view is restored");
    }
    assert!(store.has_attachments());
    assert_ne!(store.facts_source(), "facts");
}

#[test]
fn an_unattached_store_never_suspends_anything() {
    let f = fixture();
    let store = Store::open(&f.main).unwrap();
    assert!(store.attached_graphs().is_empty());
    for query in LOCAL {
        assert!(!composes(&store, query), "{query}");
    }
}

/// Production's shape: a graph frozen in-process into a pack. The live
/// freeze and thaw rebuild the attached graph set with the composed SQL.
#[test]
fn a_live_freeze_and_thaw_keep_the_attached_graph_set_current() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.db");
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    let board = store.graph_create("http://example.org/board").unwrap();
    seed(&mut store, board, r#"ex:a a ex:Issue ; ex:id "a" ."#);
    let frozen = store.graph_create("http://example.org/frozen").unwrap();
    seed(&mut store, frozen, r#"ex:z a ex:Issue ; ex:id "z" ."#);
    let local = "SELECT ?id WHERE { GRAPH ex:board { ?s ex:id ?id } }";
    let archived = "SELECT ?id WHERE { GRAPH ex:frozen { ?s ex:id ?id } }";
    let before = (rows(&store, local), rows(&store, archived));
    assert!(store.attached_graphs().is_empty());

    store
        .freeze_graph(
            "http://example.org/frozen",
            dir.path().to_str().unwrap(),
            "2026-10-08T00:00:01Z",
            Some("tester"),
        )
        .unwrap();
    assert!(store.has_attachments());
    assert!(!store.attached_graphs().is_empty());
    assert!(!composes(&store, local));
    assert!(composes(&store, archived));
    assert_eq!((rows(&store, local), rows(&store, archived)), before);

    store
        .thaw_graph(
            "http://example.org/frozen",
            "2026-10-08T00:00:02Z",
            Some("tester"),
        )
        .unwrap();
    assert!(store.attached_graphs().is_empty());
    assert_eq!((rows(&store, local), rows(&store, archived)), before);
}

/// A local fact may name a term by its attached id (a write that bound a
/// value read from the layer). Only the composed paths canonicalise it, so a
/// scope holding one keeps the composed view and still deduplicates.
#[test]
fn a_local_fact_naming_an_alias_keeps_the_composed_view() {
    let f = fixture();
    let mut store = attached(&f);
    let query = "SELECT ?id WHERE { GRAPH ex:board { ?s ex:id ?id } }";
    assert!(!composes(&store, query));
    let issue = store.lookup("http://example.org/Issue").unwrap().unwrap();
    let alias: i64 = store
        .conn
        .query_row(
            "SELECT alias_id FROM term_alias WHERE canonical_id = ?1",
            [issue],
            |r| r.get(0),
        )
        .unwrap();
    let board = store.lookup("http://example.org/board").unwrap().unwrap();
    let a = store.lookup("http://example.org/a").unwrap().unwrap();
    let rdf_type = store.intern(crate::namespace::RDF_TYPE).unwrap();
    store
        .transact_to_graph(
            &[crate::store::Datum {
                entity: a,
                attribute: rdf_type,
                value: crate::types::Value::Ref(alias),
                valid_from: TS.into(),
                valid_to: None,
                op: crate::types::Op::Assert,
            }],
            TS,
            None,
            None,
            board,
        )
        .unwrap();
    assert!(composes(&store, query));
    let typed = "SELECT ?s WHERE { GRAPH ex:board { ?s a ex:Issue } }";
    assert!(composes(&store, typed));
    assert_eq!(rows(&store, typed), rows_in_view(&store, typed, true));
}

/// The probe runs on every query against a composed store, so each arm must
/// seek its own index; on the production board a graph-prefix plan took
/// over a minute.
#[test]
fn the_alias_probe_seeks_one_index_per_arm() {
    let f = fixture();
    let store = attached(&f);
    let mut stmt = store
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {}", super::ALIAS_PROBE))
        .unwrap();
    let plan: Vec<String> = stmt
        .query_map(rusqlite::params![1_i64, 2_i64, vec![0_u8; 9]], |r| {
            r.get::<_, String>(3)
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let plan = plan.join("\n");
    assert!(plan.contains("idx_geav (g=? AND e=?)"), "{plan}");
    assert!(plan.contains("idx_aevt (a=?)"), "{plan}");
    assert!(plan.contains("idx_vaet (v=?)"), "{plan}");
    assert!(!plan.contains("idx_geav (g=?)\n"), "{plan}");
}
