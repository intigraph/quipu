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
    static TOP_DISABLED: Cell<bool> = const { Cell::new(false) };
    static DROVE: Cell<usize> = const { Cell::new(0) };
    static TOPPED: Cell<usize> = const { Cell::new(0) };
    static PROBED: Cell<usize> = const { Cell::new(0) };
    static ESTIMATED: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn estimated() {
    ESTIMATED.set(ESTIMATED.get() + 1);
}

/// Rows and how many predicate counts the driver paid for them.
pub(super) fn estimated_run(store: &Store, text: &str) -> (crate::sparql::QueryResult, usize) {
    ESTIMATED.set(0);
    let result = query(store, text).unwrap();
    (result, ESTIMATED.get())
}

pub(super) fn disabled() -> bool {
    DISABLED.get()
}

pub(super) fn top_disabled() -> bool {
    TOP_DISABLED.get()
}

pub(super) fn drove() {
    DROVE.set(DROVE.get() + 1);
}

pub(super) fn topped() {
    TOPPED.set(TOPPED.get() + 1);
}

pub(super) fn probed() {
    PROBED.set(PROBED.get() + 1);
}

fn with<T>(flag: &'static std::thread::LocalKey<Cell<bool>>, run: impl FnOnce() -> T) -> T {
    struct Reset(&'static std::thread::LocalKey<Cell<bool>>, bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            self.0.set(self.1);
        }
    }
    let _reset = Reset(flag, flag.replace(true));
    run()
}

pub(super) fn reference<T>(run: impl FnOnce() -> T) -> T {
    with(&DISABLED, run)
}

/// The driven plan as it was before the top `k`: every survivor probed.
pub(super) fn untopped<T>(run: impl FnOnce() -> T) -> T {
    with(&TOP_DISABLED, run)
}

/// Rows and how many times the driven plan ran for them.
fn driven(store: &Store, text: &str) -> (crate::sparql::QueryResult, usize) {
    DROVE.set(0);
    let result = query(store, text).unwrap();
    (result, DROVE.get())
}

/// Rows, how many times the top `k` ran, and how many survivors it probed.
pub(super) fn topped_run(
    store: &Store,
    text: &str,
) -> (
    crate::error::Result<crate::sparql::QueryResult>,
    usize,
    usize,
) {
    TOPPED.set(0);
    PROBED.set(0);
    let result = query(store, text);
    (result, TOPPED.get(), PROBED.get())
}

pub(super) fn bag(rows: &[Bindings]) -> Vec<String> {
    let mut result: Vec<_> = rows.iter().map(row_text).collect();
    result.sort();
    result
}

pub(super) fn row_text(row: &Bindings) -> String {
    let mut values: Vec<_> = row.iter().collect();
    values.sort_by_key(|(k, _)| *k);
    format!("{values:?}")
}

pub(super) const BOARD: &str = "http://example.org/board";
const DT: &str = "http://www.w3.org/2001/XMLSchema#dateTime";

pub(super) fn put(store: &mut Store, graph: &str, data: &str) {
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

pub(super) fn fixture(items: usize, composed: bool) -> (Store, tempfile::TempDir) {
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

pub(super) const PREFIX: &str = "PREFIX s: <https://schema.org/> PREFIX q: <https://scbrown.github.io/quechua/ns#> PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> ";

pub(super) fn closes(filter: &str) -> String {
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

// Volatile expressions must not drive. Unlike the deterministic cases above,
// two NOW queries need not return equal bags: they may straddle an end time.
#[test]
fn now_only_filter_declines_without_comparing_separate_clock_instants() {
    let (store, _dir) = fixture(12_000, true);
    let text = plain("?end <= NOW()");
    assert_eq!(driven(&store, &text).1, 0, "{text}");

    // Make volatility, rather than today's selectivity, the reason to decline.
    // Both IF branches return the same cutoff, so even two separate NOW
    // evaluations cannot change this answer. Removing NOW from the volatility
    // exclusion would incorrectly drive this selective filter.
    let cutoff = "\"2026-10-27T12:00:00Z\"^^xsd:dateTime";
    let text = plain(&format!("?end >= IF(NOW() = NOW(), {cutoff}, {cutoff})"));
    let old = reference(|| query(&store, &text).unwrap());
    let (new, drove) = driven(&store, &text);
    assert_eq!(drove, 0, "volatile selective filter: {text}");
    assert_eq!(bag(new.rows()), bag(old.rows()), "{text}");
    assert!(!new.rows().is_empty());
    assert!(new.rows().len() < 12_000 / 4);
}

#[test]
fn end_time_boundary_changes_the_bag_without_changing_plan_parity() {
    // Reproduce the 2026-10-26 05:00 UTC CI boundary without reading or changing
    // the wall clock. Each instant has its own independent reference answer.
    for composed in [false, true] {
        let (store, _dir) = fixture(12_000, composed);
        let mut answers = Vec::new();
        for instant in ["2026-10-26T04:59:59.999Z", "2026-10-26T05:00:00Z"] {
            let text = plain(&format!("?end <= \"{instant}\"^^xsd:dateTime"));
            let old = reference(|| query(&store, &text).unwrap());
            let (new, drove) = driven(&store, &text);
            assert_eq!(drove, 0, "nonselective boundary: {text}");
            assert_eq!(new.variables(), old.variables(), "{text}");
            let rows = bag(new.rows());
            assert_eq!(rows, bag(old.rows()), "composed={composed} {text}");
            assert!(!rows.is_empty(), "positive control: {text}");
            answers.push(rows);
        }
        let before = &answers[0];
        let after = &answers[1];
        assert!(before.iter().all(|row| after.binary_search(row).is_ok()));
        let entering: Vec<_> = after
            .iter()
            .filter(|row| before.binary_search(row).is_err())
            .collect();
        assert_eq!(after.len(), before.len() + 46);
        assert_eq!(entering.len(), 46, "the bags must differ at this boundary");
        assert!(
            entering
                .iter()
                .all(|row| row.contains("2026-10-26T05:00:00Z"))
        );
    }
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

const FLAG: &str = "urn:review:flag";
const RX: &str = "urn:review:regex";

/// 5,000 flagged items, the first with a valid pattern, and `bad` on the
/// subject `bad_on` (unflagged unless `bad_on` is a flagged item).
fn regex_store(bad_on: &str) -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("rx.db").to_str().unwrap()).unwrap();
    let mut data = String::new();
    for i in 0..5000 {
        data.push_str(&format!("<urn:review:item/{i}> <{FLAG}> \"yes\" .\n"));
    }
    data.push_str(&format!("<urn:review:item/0> <{RX}> \"x\" .\n"));
    data.push_str(&format!("<{bad_on}> <{RX}> \"[\" .\n"));
    put(&mut store, BOARD, &data);
    // The resident read model joins in memory and is never driven.
    store.set_read_model_enabled(false);
    (store, dir)
}

fn rx_select() -> String {
    format!(
        "SELECT ?item WHERE {{ GRAPH <{BOARD}> {{ ?item <{FLAG}> \"yes\" ; <{RX}> ?rx . \
         FILTER(REGEX(\"x\", ?rx)) }} }}"
    )
}

fn rx_count() -> String {
    format!(
        "SELECT (COUNT(DISTINCT ?item) AS ?n) WHERE {{ GRAPH <{BOARD}> {{ \
         ?item <{FLAG}> \"yes\" ; <{RX}> ?rx . FILTER(REGEX(\"x\", ?rx)) }} }}"
    )
}

/// Review of #733: a hard error on a row the join discards must not fail the
/// query. The original plan never evaluates the filter on the unflagged
/// subject; the driven plan judged it before the join and raised its invalid
/// pattern. Now the error is deferred to joined rows, and both plans answer.
#[test]
fn an_error_on_a_row_the_join_discards_does_not_fail_the_query() {
    let (store, _dir) = regex_store("urn:review:loner");
    for text in [rx_select(), rx_count()] {
        let expected = reference(|| query(&store, &text).unwrap());
        let (actual, drove) = driven(&store, &text);
        assert!(
            drove > 0,
            "the driven plan must run, or this proves nothing: {text}"
        );
        assert_eq!(bag(actual.rows()), bag(expected.rows()), "{text}");
    }
    assert_eq!(bag(query(&store, &rx_select()).unwrap().rows()).len(), 1);
}

/// The positive control: on a row that DOES join, the error is genuine and
/// both plans refuse. Swallowing every pattern error is not the repair.
#[test]
fn an_error_on_a_joined_row_still_fails_both_plans() {
    let (store, _dir) = regex_store("urn:review:item/1");
    for text in [rx_select(), rx_count()] {
        assert!(
            reference(|| query(&store, &text)).is_err(),
            "original: {text}"
        );
        DROVE.set(0);
        assert!(query(&store, &text).is_err(), "driven: {text}");
        assert!(DROVE.get() > 0, "the driven plan must have run: {text}");
    }
}

/// Only a leading run of conjuncts judges early. Here the first conjunct
/// needs both patterns, so no single pattern can judge the prefix and the
/// original plan runs, even though the second conjunct alone could drive.
#[test]
fn a_conjunct_after_one_the_driver_cannot_judge_is_not_applied_early() {
    let (store, _dir) = regex_store("urn:review:loner");
    let text = format!(
        "SELECT ?item WHERE {{ GRAPH <{BOARD}> {{ ?item <{FLAG}> ?f ; <{RX}> ?rx . \
         FILTER(?f != ?rx && REGEX(\"x\", ?rx)) }} }}"
    );
    let expected = reference(|| query(&store, &text).unwrap());
    let (actual, drove) = driven(&store, &text);
    assert_eq!(drove, 0, "a non-prefix conjunct must not drive");
    assert_eq!(bag(actual.rows()), bag(expected.rows()));
}
