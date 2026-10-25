//! The top `k` under ORDER BY LIMIT: the rows the full driven join returns,
//! after probing only the survivors the ordering can reach (aegis-xacr).
use super::tests::{
    BOARD, PREFIX, closes, fixture, put, reference, row_text, topped_run, untopped,
};
use crate::sparql::{QueryResult, query};
use crate::store::Store;

const WIDE: &str = "?end >= \"2026-01-01T00:00:00Z\"^^xsd:dateTime";
const NARROW: &str = "?end >= \"2026-10-27T12:00:00Z\"^^xsd:dateTime";

fn ordered(result: &QueryResult) -> Vec<String> {
    result.rows().iter().map(row_text).collect()
}

/// The `?end` column in result order: the answer when rows tie on it.
fn ends(result: &QueryResult) -> Vec<String> {
    result
        .rows()
        .iter()
        .map(|row| format!("{:?}", row.get("end")))
        .collect()
}

fn newest(filter: &str, limit: usize) -> String {
    format!(
        "{PREFIX}SELECT ?item ?end WHERE {{ GRAPH <{BOARD}> {{ ?item q:outcome \"done\" ; \
         s:endTime ?end . FILTER({filter}) }} }} ORDER BY DESC(?end) LIMIT {limit}"
    )
}

/// The Brief's closes panel over a window that keeps most of the board: the
/// half-the-driver test declines it, so before the top `k` every done item
/// was probed. Keys (?end, ?item) break every tie, so the order is the answer.
#[test]
fn a_wide_window_probes_only_the_rows_the_limit_returns() {
    for composed in [false, true] {
        let (store, _dir) = fixture(12_000, composed);
        let text = closes(WIDE);
        let old = reference(|| query(&store, &text).unwrap());
        let (new, topped, probed) = topped_run(&store, &text);
        let new = new.unwrap();
        assert_eq!(topped, 1, "the top k must run: composed={composed}");
        assert_eq!(new.variables(), old.variables());
        assert_eq!(ordered(&new), ordered(&old), "composed={composed}");
        assert_eq!(new.rows().len(), 101);
        // One item in seven is open and fails the join; ~9,000 survive.
        assert!(probed < 400, "probed {probed} survivors");
    }
}

/// Without OPTIONAL there is no #700 rewrite: the hint reaches the filter
/// through the named graph. Rows tie on `?end` alone (hourly end times), and
/// the limit cuts through a tie. Where the full driven join runs, the top `k`
/// returns its rows in its order, ties included; where the original plan runs
/// instead, ties may come back in another order, so the key column is the
/// answer.
#[test]
fn ties_at_the_limit_keep_the_full_join_answer() {
    let (store, _dir) = fixture(12_000, false);
    for limit in [1, 7, 50, 101] {
        let text = newest(NARROW, limit);
        let full = untopped(|| query(&store, &text).unwrap());
        let (new, topped, _) = topped_run(&store, &text);
        let new = new.unwrap();
        assert_eq!(topped, 1, "{text}");
        assert_eq!(ordered(&new), ordered(&full), "{text}");
        assert_eq!(new.rows().len(), limit);

        let text = newest(WIDE, limit);
        let old = reference(|| query(&store, &text).unwrap());
        let (new, topped, probed) = topped_run(&store, &text);
        let new = new.unwrap();
        assert_eq!(topped, 1, "{text}");
        assert_eq!(ends(&new), ends(&old), "{text}");
        assert!(probed < 400, "probed {probed}: {text}");
    }
    // LIMIT 0 answers nothing, as before.
    let (new, _, _) = topped_run(&store, &newest(WIDE, 0));
    assert!(new.unwrap().rows().is_empty());
}

/// Each case would make a top `k` wrong or unprovable, so the driven plan
/// runs as it did, and the answer equals the original plan's.
#[test]
fn declines_where_the_order_cannot_be_trusted() {
    let (store, _dir) = fixture(12_000, false);
    for text in [
        // A key the driver does not bind: `?o` comes from another pattern.
        format!(
            "{PREFIX}SELECT ?item ?o WHERE {{ GRAPH <{BOARD}> {{ ?item q:outcome ?o ; \
             s:endTime ?end . FILTER({NARROW}) }} }} ORDER BY ?o ?item LIMIT 5"
        ),
        // A conjunct the driver cannot judge stays above the join.
        format!(
            "{PREFIX}SELECT ?item ?end WHERE {{ GRAPH <{BOARD}> {{ ?item q:outcome ?o ; \
             s:endTime ?end . FILTER({NARROW} && ?o = \"done\") }} }} \
             ORDER BY DESC(?end) ?item LIMIT 5"
        ),
        // A volatile key.
        format!(
            "{PREFIX}SELECT ?item WHERE {{ GRAPH <{BOARD}> {{ ?item q:outcome \"done\" ; \
             s:endTime ?end . FILTER({NARROW}) }} }} ORDER BY (RAND() < 2) ?item LIMIT 5"
        ),
    ] {
        let old = reference(|| query(&store, &text).unwrap());
        let (new, topped, _) = topped_run(&store, &text);
        assert_eq!(topped, 0, "{text}");
        assert_eq!(ordered(&new.unwrap()), ordered(&old), "{text}");
    }
}

/// End times of mixed comparison classes (integers beside dateTimes) are not
/// totally ordered, so the top `k` declines and the answer is unchanged.
#[test]
fn mixed_key_classes_decline() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("mixed.db").to_str().unwrap()).unwrap();
    let mut data = String::new();
    for i in 0..6000 {
        let s = format!("<http://example.org/item/{i}>");
        data.push_str(&format!(
            "{s} <https://scbrown.github.io/quechua/ns#outcome> \"done\" .\n"
        ));
        let end = if i % 2 == 0 {
            format!("\"{i}\"^^<http://www.w3.org/2001/XMLSchema#integer>")
        } else {
            format!("\"x{i}\"")
        };
        data.push_str(&format!("{s} <https://schema.org/endTime> {end} .\n"));
    }
    put(&mut store, BOARD, &data);
    store.set_read_model_enabled(false);
    let text = newest("BOUND(?end)", 5);
    let old = reference(|| query(&store, &text).unwrap());
    let (new, topped, _) = topped_run(&store, &text);
    assert_eq!(topped, 0, "mixed classes must decline");
    assert_eq!(ends(&new.unwrap()), ends(&old));
}

/// A filter that raises on a joined row fails the original plan. The top `k`
/// would never reach that row, so it must decline, and the query still fails.
/// The positive control: the same ordering with a valid pattern does run.
#[test]
fn a_raising_filter_declines_and_still_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("rx.db").to_str().unwrap()).unwrap();
    let mut data = String::new();
    for i in 0..6000 {
        let s = format!("<urn:review:item/{i}>");
        data.push_str(&format!("{s} <urn:review:flag> \"yes\" .\n"));
        let rx = if i == 4321 { "[" } else { "x" };
        data.push_str(&format!("{s} <urn:review:regex> \"{rx}\" .\n"));
    }
    put(&mut store, BOARD, &data);
    store.set_read_model_enabled(false);
    let text = format!(
        "SELECT ?item WHERE {{ GRAPH <{BOARD}> {{ ?item <urn:review:flag> \"yes\" ; \
         <urn:review:regex> ?rx . FILTER(REGEX(\"x\", ?rx)) }} }} ORDER BY ?item LIMIT 3"
    );
    assert!(reference(|| query(&store, &text)).is_err());
    let (new, topped, _) = topped_run(&store, &text);
    assert!(new.is_err(), "a raised error must not be hidden");
    assert_eq!(topped, 0);
    let valid = format!(
        "SELECT ?item WHERE {{ GRAPH <{BOARD}> {{ ?item <urn:review:flag> \"yes\" ; \
         <urn:review:regex> ?rx . FILTER(?rx = \"x\") }} }} ORDER BY ?item LIMIT 3"
    );
    let old = reference(|| query(&store, &valid).unwrap());
    let (new, topped, _) = topped_run(&store, &valid);
    assert_eq!(topped, 1, "positive control: the top k runs");
    assert_eq!(ordered(&new.unwrap()), ordered(&old));
}

/// A survivor the filter cannot judge (an IRI has no effective boolean value,
/// so `FILTER(?flag)` is a type error on it) is dropped by the caller's
/// filter, so it must not take a place in the top `k`. Here those survivors
/// sort first by `?item`; ordering them would fill the limit with rows the
/// filter then removes. (Comparisons such as `>=` are two-valued here: an
/// ill-typed end time is judged `false`, so it cannot reach this case.)
#[test]
fn rows_the_filter_cannot_judge_take_no_place_in_the_top() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("unknown.db").to_str().unwrap()).unwrap();
    let mut data = String::new();
    for i in 0..6000 {
        let (s, flag) = if i < 20 {
            (
                format!("<http://example.org/a/{i:04}>"),
                "<http://example.org/maybe>",
            )
        } else {
            (
                format!("<http://example.org/b/{i:04}>"),
                "\"true\"^^<http://www.w3.org/2001/XMLSchema#boolean>",
            )
        };
        data.push_str(&format!(
            "{s} <https://scbrown.github.io/quechua/ns#outcome> \"done\" .\n\
             {s} <urn:test:flag> {flag} .\n"
        ));
    }
    put(&mut store, BOARD, &data);
    store.set_read_model_enabled(false);
    let text = format!(
        "{PREFIX}SELECT ?item WHERE {{ GRAPH <{BOARD}> {{ ?item q:outcome \"done\" ; \
         <urn:test:flag> ?flag . FILTER(?flag) }} }} ORDER BY ?item LIMIT 5"
    );
    let old = reference(|| query(&store, &text).unwrap());
    let (new, topped, _) = topped_run(&store, &text);
    let new = new.unwrap();
    assert_eq!(topped, 1, "positive control: the top k runs");
    assert_eq!(old.rows().len(), 5);
    assert_eq!(ordered(&new), ordered(&old));
}
