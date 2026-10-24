//! Filter placement: the same rows with and without it, and placement only
//! where every row it filters still reaches the original filter.
use std::cell::Cell;

use oxrdfio::RdfFormat;

use crate::rdf::ingest_rdf_to_graph;
use crate::sparql::{Bindings, QueryResult, query};
use crate::store::Store;

thread_local! {
    static DISABLED: Cell<bool> = const { Cell::new(false) };
    static PLACED: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn disabled() -> bool {
    DISABLED.get()
}

pub(super) fn placed_once() {
    PLACED.set(PLACED.get() + 1);
}

const MAIN: &str = "http://example.org/board";
const SIDE: &str = "http://example.org/board#side";
const DT: &str = "http://www.w3.org/2001/XMLSchema#dateTime";
const INT: &str = "http://www.w3.org/2001/XMLSchema#integer";
const S: &str = "https://schema.org/";

fn put(store: &mut Store, graph: &str, data: &str) {
    let graph = store.graph_create(graph).unwrap();
    ingest_rdf_to_graph(
        store,
        data.as_bytes(),
        RdfFormat::NTriples,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
        graph,
    )
    .unwrap();
}

/// Items closed across three months on two graphs; some lack the optional
/// properties, some carry the inbox keyword, one has an ill-typed end time.
fn fixture() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let mut main = String::new();
    let mut side = String::new();
    for i in 0..60 {
        let month = 9 + i % 3;
        let day = 1 + i % 28;
        let out = if i % 5 == 0 { &mut side } else { &mut main };
        let s = format!("<http://example.org/item/{i}>");
        out.push_str(&format!(
            "{s} <{S}endTime> \"2026-{month:02}-{day:02}T10:00:00Z\"^^<{DT}> .\n\
             {s} <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <{S}Action> .\n"
        ));
        if i % 2 == 0 {
            out.push_str(&format!("{s} <{S}name> \"item {i}\" .\n"));
        }
        if i % 3 == 0 {
            out.push_str(&format!(
                "{s} <https://seeds.local/ontology/priority> \"{}\"^^<{INT}> .\n",
                i % 5
            ));
        }
        if i % 7 == 0 {
            out.push_str(&format!("{s} <{S}keywords> \"inbox\" .\n"));
        }
    }
    main.push_str(&format!(
        "<http://example.org/item/bad> <{S}endTime> \"not a date\" .\n\
         <http://example.org/item/bad> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <{S}Action> .\n"
    ));
    put(&mut store, MAIN, &main);
    put(&mut store, SIDE, &side);
    store
}

fn bag(result: &QueryResult) -> Vec<String> {
    let QueryResult::Select { rows, .. } = result else {
        panic!("expected SELECT");
    };
    let mut out: Vec<String> = rows.iter().map(row_text).collect();
    out.sort();
    out
}

fn row_text(row: &Bindings) -> String {
    let mut values: Vec<_> = row.iter().collect();
    values.sort_by_key(|(k, _)| *k);
    format!("{values:?}")
}

/// Both plans' rows, and how many placements the placed plan made.
fn both(store: &Store, text: &str) -> (Vec<String>, Vec<String>, usize) {
    DISABLED.set(true);
    let original = query(store, text).map(|r| bag(&r));
    DISABLED.set(false);
    PLACED.set(0);
    let placed = query(store, text).map(|r| bag(&r));
    let count = PLACED.get();
    match (original, placed) {
        (Ok(a), Ok(b)) => (a, b, count),
        (Err(a), Err(b)) => (vec![format!("ERR {a}")], vec![format!("ERR {b}")], count),
        (a, b) => panic!("plans disagree on failure: {a:?} vs {b:?}"),
    }
}

fn branch(graph: &str) -> String {
    format!(
        "{{ GRAPH <{graph}> {{ ?s <{S}endTime> ?stamp . ?s a <{S}Action> . \
         OPTIONAL {{ ?s <{S}name> ?title }} \
         OPTIONAL {{ ?s <https://seeds.local/ontology/priority> ?p }} \
         OPTIONAL {{ ?s <{S}keywords> ?inbox FILTER(LCASE(STR(?inbox))='inbox') }} }} }}"
    )
}

/// The aggregate shape: FILTER above BIND, UNION, GRAPH and OPTIONALs.
fn month_stats(filter: &str) -> String {
    format!(
        "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> \
         SELECT ?kind ?day (COUNT(DISTINCT ?s) AS ?n) WHERE {{ \
         {{ {} UNION {} }} \
         FILTER({filter}) \
         BIND(IF(BOUND(?inbox),'inbox','work') AS ?kind) \
         BIND(SUBSTR(STR(?stamp),1,10) AS ?day) \
         }} GROUP BY ?kind ?day",
        branch(MAIN),
        branch(SIDE)
    )
}

const OCTOBER: &str = "?stamp >= xsd:dateTime('2026-10-01T00:00:00Z') && ?stamp < xsd:dateTime('2026-11-01T00:00:00Z')";

#[test]
fn the_month_aggregate_is_placed_and_answers_the_same() {
    let store = fixture();
    let (original, placed, count) = both(&store, &month_stats(OCTOBER));
    assert!(!original.is_empty());
    assert_eq!(original, placed);
    assert!(count >= 1, "the filter was not placed");
}

#[test]
fn a_filter_on_an_optional_only_variable_is_not_placed() {
    let store = fixture();
    for filter in ["?p = 1", "BOUND(?title)", "!BOUND(?inbox)"] {
        let (original, placed, count) = both(&store, &month_stats(filter));
        assert_eq!(original, placed, "{filter}");
        assert_eq!(
            count, 0,
            "{filter} reads a variable OPTIONAL may leave unbound"
        );
    }
}

#[test]
fn a_filter_on_a_bound_variable_is_not_placed_below_its_bind() {
    let store = fixture();
    let (original, placed, count) = both(&store, &month_stats("?kind = 'inbox'"));
    assert_eq!(original, placed);
    assert_eq!(count, 0);
}

#[test]
fn only_a_stable_prefix_is_copied_and_a_late_hard_error_keeps_its_fate() {
    let store = fixture();
    // The first conjunct is placeable; the invalid REGEX after it is not copied,
    // so its hard error surfaces exactly where the original raises it (or not).
    let filter = format!("{OCTOBER} && REGEX(STR(?s), '(')");
    let (original, placed, _) = both(&store, &month_stats(&filter));
    assert_eq!(original, placed);
    // A volatile or EXISTS conjunct first: nothing is copied.
    let (o2, p2, c2) = both(
        &store,
        &month_stats("RAND() < 2 && ?stamp > xsd:dateTime('2026-01-01T00:00:00Z')"),
    );
    assert_eq!(o2, p2);
    assert_eq!(c2, 0);
}

#[test]
fn a_variable_bound_in_only_one_union_branch_is_not_placed() {
    let store = fixture();
    let q = format!(
        "SELECT ?s WHERE {{ {{ GRAPH <{MAIN}> {{ ?s <{S}name> ?title }} }} UNION \
         {{ GRAPH <{SIDE}> {{ ?s <{S}endTime> ?stamp }} }} FILTER(BOUND(?title) || ?title = 'x') }}"
    );
    let (original, placed, count) = both(&store, &q);
    assert_eq!(original, placed);
    assert_eq!(count, 0);
}
