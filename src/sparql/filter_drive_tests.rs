//! Filter-driven BGP: the same rows as the original plan, taken only when it
//! cannot lose (aegis-xacr).
use std::cell::Cell;

use oxrdfio::RdfFormat;

use crate::rdf::ingest_rdf_to_graph;
use crate::sparql::{Bindings, query};
use crate::store::Store;
use crate::store::attach::Attachment;

thread_local! {
    static DISABLED: Cell<bool> = const { Cell::new(false) };
    static DROVE: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn disabled() -> bool {
    DISABLED.get()
}

pub(super) fn drove() {
    DROVE.set(DROVE.get() + 1);
}

fn reference<T>(run: impl FnOnce() -> T) -> T {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            DISABLED.set(self.0);
        }
    }
    let _reset = Reset(DISABLED.replace(true));
    run()
}

/// Rows and how many times the driven plan ran for them.
fn driven(store: &Store, text: &str) -> (crate::sparql::QueryResult, usize) {
    DROVE.set(0);
    let result = query(store, text).unwrap();
    (result, DROVE.get())
}

fn bag(rows: &[Bindings]) -> Vec<String> {
    let mut result: Vec<_> = rows.iter().map(row_text).collect();
    result.sort();
    result
}

fn row_text(row: &Bindings) -> String {
    let mut values: Vec<_> = row.iter().collect();
    values.sort_by_key(|(k, _)| *k);
    format!("{values:?}")
}

const BOARD: &str = "http://example.org/board";
const DT: &str = "http://www.w3.org/2001/XMLSchema#dateTime";

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

/// `items` closed items with end times spread over days, plus the cases a
/// filter treats specially: ill-typed, offset and fractional end times, two
/// end times on one item, items still open, items with no end time, and
/// end times on another graph.
fn board(items: usize) -> String {
    let mut data = String::new();
    for i in 0..items {
        let s = format!("<http://example.org/item/{i}>");
        let outcome = if i % 7 == 0 { "open" } else { "done" };
        data.push_str(&format!(
            "{s} <https://scbrown.github.io/quechua/ns#outcome> \"{outcome}\" .\n"
        ));
        let day = 1 + i % 28;
        let hour = i % 24;
        let end = match i % 11 {
            0 => continue,
            1 => format!("2026-10-{day:02}T{hour:02}:00:00+05:00"),
            2 => format!("2026-10-{day:02}T{hour:02}:30:00.250Z"),
            3 => "not-a-date".to_string(),
            _ => format!("2026-10-{day:02}T{hour:02}:00:00Z"),
        };
        data.push_str(&format!(
            "{s} <https://schema.org/endTime> \"{end}\"^^<{DT}> .\n"
        ));
        if i % 13 == 0 {
            data.push_str(&format!(
                "{s} <https://schema.org/endTime> \"2026-10-27T00:00:00Z\"^^<{DT}> .\n"
            ));
        }
        if i % 3 == 0 {
            data.push_str(&format!("{s} <https://schema.org/name> \"item {i}\" .\n"));
        }
    }
    data
}

fn fixture(items: usize, composed: bool) -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.db");
    {
        let mut store = Store::open(main.to_str().unwrap()).unwrap();
        put(&mut store, BOARD, &board(items));
        put(
            &mut store,
            "http://example.org/elsewhere",
            &format!(
                "<http://example.org/item/1> <https://schema.org/endTime> \"2026-10-28T00:00:00Z\"^^<{DT}> .\n"
            ),
        );
    }
    let store = if composed {
        let raw = dir.path().join("raw.db");
        {
            let mut provider = Store::open(raw.to_str().unwrap()).unwrap();
            put(
                &mut provider,
                "urn:test:archive",
                "<urn:test:a> <urn:test:p> \"archived\" .\n",
            );
        }
        let pack = dir.path().join("archive.db");
        crate::store::respace::respace_file(&raw, &pack, 1).unwrap();
        Store::open_with_attachments(
            main.to_str().unwrap(),
            &[Attachment::read_only("archive", pack.to_str().unwrap())],
        )
        .unwrap()
    } else {
        Store::open(main.to_str().unwrap()).unwrap()
    };
    store.set_read_model_enabled(false);
    (store, dir)
}

const PREFIX: &str = "PREFIX s: <https://schema.org/> PREFIX q: <https://scbrown.github.io/quechua/ns#> PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> ";

fn closes(filter: &str) -> String {
    format!(
        "{PREFIX}SELECT ?item ?title ?end WHERE {{ GRAPH <{BOARD}> {{ ?item q:outcome \"done\" ; s:endTime ?end . FILTER({filter}) OPTIONAL {{ ?item s:name ?title }} }} }} ORDER BY DESC(?end) ?item LIMIT 101"
    )
}

fn plain(filter: &str) -> String {
    format!(
        "{PREFIX}SELECT * WHERE {{ GRAPH <{BOARD}> {{ ?item q:outcome \"done\" ; s:endTime ?end . FILTER({filter}) }} }}"
    )
}

#[test]
fn driven_rows_match_the_original_plan_on_both_providers() {
    let window = "?end >= \"2026-10-27T12:00:00Z\"^^xsd:dateTime";
    for composed in [false, true] {
        let (store, _dir) = fixture(12_000, composed);
        for text in [
            closes(window),
            plain(window),
            // A conjunct the end time cannot judge stays above the join.
            plain(&format!("{window} && ?item != <http://example.org/item/9>")),
            // Offsets and fractions compare by value, not text.
            plain("?end > \"2026-10-27T18:00:00+05:00\"^^xsd:dateTime"),
            plain(&format!("{window} && STRSTARTS(STR(?end), \"2026-10-2\")")),
        ] {
            let old = reference(|| query(&store, &text).unwrap());
            let (new, drove) = driven(&store, &text);
            assert_eq!(drove, 1, "driven plan must run: composed={composed} {text}");
            assert_eq!(new.variables(), old.variables(), "{text}");
            assert_eq!(
                bag(new.rows()),
                bag(old.rows()),
                "composed={composed} {text}"
            );
            assert!(!new.rows().is_empty(), "positive control: {text}");
            assert!(
                new.rows().len() < 12_000 / 4,
                "the window is selective: {text}"
            );
        }
        // ORDER BY ... LIMIT with keys that break every tie returns the same
        // rows in the same order.
        let text = closes(window);
        let old = reference(|| query(&store, &text).unwrap());
        let new = query(&store, &text).unwrap();
        let ordered =
            |r: &crate::sparql::QueryResult| r.rows().iter().map(row_text).collect::<Vec<_>>();
        assert_eq!(ordered(&new), ordered(&old));
        assert_eq!(new.rows().len(), 101);
    }
}

#[test]
fn ill_typed_and_two_valued_end_times_keep_their_original_fate() {
    let (store, _dir) = fixture(12_000, true);
    // Item 3 has an ill-typed end time (an error, so eliminated); item 13 has
    // two, one of them inside the window (kept once, for that value only).
    let text = plain("?end >= \"2026-10-27T00:00:00Z\"^^xsd:dateTime");
    let (rows, drove) = driven(&store, &text);
    assert_eq!(drove, 1);
    // Rows show terms by id, so check values: no ill-typed end time survives,
    // and every second end time inside the window is kept.
    let items: Vec<String> = rows.rows().iter().map(row_text).collect();
    let old = reference(|| query(&store, &text).unwrap());
    let old_items: Vec<String> = old.rows().iter().map(row_text).collect();
    // The ill-typed value meets the same fate on both plans.
    let ill = |rows: &[String]| rows.iter().filter(|r| r.contains("not-a-date")).count();
    assert_eq!(ill(&items), ill(&old_items));
    let extra = (0..12_000)
        .filter(|i| i % 13 == 0 && i % 11 != 0 && i % 7 != 0)
        .count();
    let at_midnight = items
        .iter()
        .filter(|r| r.contains("2026-10-27T00:00:00Z"))
        .count();
    assert!(at_midnight >= extra, "{at_midnight} < {extra}");
    assert_eq!(bag(rows.rows()), bag(old.rows()));
}

#[test]
fn declines_where_driving_could_cost_more_or_change_the_answer() {
    let window = "?end >= \"2026-10-27T12:00:00Z\"^^xsd:dateTime";
    // A small original driver is already cheap.
    let (small, _dir) = fixture(2_000, true);
    assert_eq!(driven(&small, &plain(window)).1, 0);
    let (store, _dir) = fixture(12_000, true);
    for text in [
        // The filter keeps most rows: the probes saved cannot pay for the scan.
        plain("?end >= \"2026-01-01T00:00:00Z\"^^xsd:dateTime"),
        // NOW is not stable, and it is the only conjunct.
        plain("?end <= NOW()"),
        // No conjunct one pattern can judge: `?elsewhere` is bound by none.
        plain(&format!("{window} || BOUND(?elsewhere)")),
    ] {
        let old = reference(|| query(&store, &text).unwrap());
        let (new, drove) = driven(&store, &text);
        assert_eq!(drove, 0, "{text}");
        assert_eq!(bag(new.rows()), bag(old.rows()), "{text}");
    }
    // EXISTS sees the whole row and RAND is not stable, so neither moves; the
    // window beside them still drives, and the answer is unchanged. A
    // disjunction over variables the end-time pattern binds is judged whole.
    for text in [
        plain(&format!("{window} || ?item = <http://example.org/item/1>")),
        plain(&format!("{window} && EXISTS {{ ?item s:name ?n }}")),
        plain(&format!("{window} && RAND() < 2")),
    ] {
        let old = reference(|| query(&store, &text).unwrap());
        let (new, drove) = driven(&store, &text);
        assert_eq!(drove, 1, "{text}");
        assert_eq!(bag(new.rows()), bag(old.rows()), "{text}");
    }
    // With a resident read model the in-memory join is kept.
    let (model, _dir) = fixture(12_000, false);
    model.set_read_model_enabled(true);
    assert_eq!(driven(&model, &plain(window)).1, 0);
}

/// The streamed COUNT path drives the same way and counts the same rows.
#[test]
fn a_filtered_count_drives_and_counts_the_same_rows() {
    let (store, _dir) = fixture(12_000, true);
    let text = format!(
        "{PREFIX}SELECT (COUNT(DISTINCT ?item) AS ?n) WHERE {{ GRAPH <{BOARD}> {{ ?item q:outcome \"done\" ; s:endTime ?end . FILTER(?end >= \"2026-10-27T12:00:00Z\"^^xsd:dateTime) }} }}"
    );
    let old = reference(|| query(&store, &text).unwrap());
    let (new, drove) = driven(&store, &text);
    assert_eq!(drove, 1, "the streamed count must drive");
    assert_eq!(bag(new.rows()), bag(old.rows()));
    assert!(!new.rows().is_empty());
}
