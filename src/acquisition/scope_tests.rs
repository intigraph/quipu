use std::sync::{Arc, Mutex};

use rusqlite::hooks::{AuthAction, AuthContext, Authorization};

use super::*;
use crate::{Op, Value, store::Datum};

const PROJECT: &str = "https://example.org/project/board";
const COMMENTS: &str = "https://example.org/project/comments";
const BUDGET: u64 = 1_000_000_000;

fn datum(store: &Store, value: Value, op: Op) -> Datum {
    Datum {
        entity: store.intern("https://example.org/item/one").unwrap(),
        attribute: store.intern("https://example.org/value").unwrap(),
        value,
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op,
    }
}

fn write(store: &mut Store, graph: &str, datums: &[Datum]) -> i64 {
    let g = store.registered_graph_id(graph).unwrap().unwrap();
    store
        .transact_to_graph(datums, "2026-01-01T00:00:00Z", None, None, g)
        .unwrap();
    store.latest_tx_id().unwrap()
}

fn request(store: &Store, graphs: &[&str], tx: Option<i64>) -> Request {
    Request {
        source_store_id: store.store_id().unwrap(),
        graphs: graphs.iter().map(|g| (*g).to_string()).collect(),
        tx,
        max_vm_steps: BUDGET,
        deadline_ms: 60_000,
    }
}

fn rows(measurement: &Measurement, iri: &str) -> u64 {
    match &measurement.graphs[iri] {
        GraphScope::Measured { rows, .. } => *rows,
        other => panic!("{iri} not measured: {other:?}"),
    }
}

/// A file store with two graphs: duplicate assertions of one value, three
/// distinct values, and a retraction after the first pin.
fn store(dir: &std::path::Path) -> (Store, i64, i64) {
    let mut store = Store::open(dir.join("main.db").to_str().unwrap()).unwrap();
    store.graph_create(PROJECT).unwrap();
    store.graph_create(COMMENTS).unwrap();
    for text in ["a", "a", "b", "c"] {
        let d = datum(&store, Value::Str(text.into()), Op::Assert);
        write(&mut store, PROJECT, &[d]);
    }
    let d = datum(&store, Value::Int(7), Op::Assert);
    let first = write(&mut store, COMMENTS, &[d]);
    let d = datum(&store, Value::Str("b".into()), Op::Retract);
    let second = write(&mut store, PROJECT, &[d]);
    (store, first, second)
}

/// The denominator is the acquisition's own: at each historical pin, measured
/// rows equal the triples a v1 acquisition returns, and at the head they equal
/// the triples the direct-main reader (`main_graph::read`, via composed) returns.
#[test]
fn measured_rows_equal_the_acquired_triples_at_each_pin() {
    let dir = tempfile::tempdir().unwrap();
    let (store, first, second) = store(dir.path());
    for pin in [first, second] {
        let acquired = super::super::acquire(
            &store,
            &super::super::Request {
                source_store_id: store.store_id().unwrap(),
                graphs: vec![PROJECT.into(), COMMENTS.into()],
                tx: Some(pin),
                max_bytes: None,
            },
        )
        .unwrap();
        let measured = measure(&store, &request(&store, &[PROJECT, COMMENTS], Some(pin))).unwrap();
        assert_eq!(measured.applied_tx, pin);
        for iri in [PROJECT, COMMENTS] {
            let triples = acquired.source_receipt.graphs[iri].triples as u64;
            assert_eq!(rows(&measured, iri), triples, "{iri} at {pin}");
        }
    }
    let composed = super::super::composed::acquire(
        &store,
        &super::super::composed::Request {
            source_store_id: store.store_id().unwrap(),
            graphs: vec![PROJECT.into(), COMMENTS.into()],
            tx: None,
            expected_generation: None,
            max_bytes: None,
        },
    )
    .unwrap();
    let head = measure(&store, &request(&store, &[PROJECT, COMMENTS], None)).unwrap();
    assert_eq!(head.applied_tx, composed.source_receipt.main.applied_tx);
    for iri in [PROJECT, COMMENTS] {
        let triples = composed.source_receipt.main.graphs[iri].triples as u64;
        assert_eq!(rows(&head, iri), triples, "{iri} at head");
    }
    // The fixture exercises both distinctness and the pin.
    let early = measure(&store, &request(&store, &[PROJECT], Some(first))).unwrap();
    let late = measure(&store, &request(&store, &[PROJECT], Some(second))).unwrap();
    assert_eq!(
        rows(&early, PROJECT),
        3,
        "four assertions, three distinct values"
    );
    assert_eq!(rows(&late, PROJECT), 2, "one retracted by the later pin");
}

#[test]
fn the_cap_is_exact_and_over_it_reports_no_count() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("main.db").to_str().unwrap()).unwrap();
    store.graph_create(PROJECT).unwrap();
    let cap = i64::try_from(ROW_CAP).unwrap();
    let datums: Vec<Datum> = (0..cap)
        .map(|i| datum(&store, Value::Int(i), Op::Assert))
        .collect();
    write(&mut store, PROJECT, &datums);
    let at_cap = measure(&store, &request(&store, &[PROJECT], None)).unwrap();
    assert_eq!(rows(&at_cap, PROJECT), ROW_CAP);
    let d = datum(&store, Value::Int(cap), Op::Assert);
    write(&mut store, PROJECT, &[d]);
    let over = measure(&store, &request(&store, &[PROJECT], None)).unwrap();
    assert_eq!(over.graphs[PROJECT], GraphScope::ExceedsCap);
    let text = serde_json::to_string(&over).unwrap();
    assert!(!text.contains("100001"), "{text}");

    // Budget exhaustion names the running phase and returns nothing. Only a
    // statement longer than one 1000-step stride is counted, so the first one
    // to cross a starved budget is the row count.
    let mut starved = request(&store, &[PROJECT], None);
    for max_vm_steps in [1, 50_000] {
        starved.max_vm_steps = max_vm_steps;
        let error = measure(&store, &starved).unwrap_err().to_string();
        assert!(error.contains("during physical_distinct"), "{error}");
        assert!(error.contains("completeness UNKNOWN"), "{error}");
    }
    // The starved handler left with the run: an unbudgeted read of every row
    // works (a leftover handler would interrupt it).
    let all: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM (SELECT DISTINCT e,a,v FROM facts)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(all, cap + 1);
    // Positive control: the same request with enough budget measures.
    starved.max_vm_steps = BUDGET;
    assert_eq!(
        measure(&store, &starved).unwrap().graphs[PROJECT],
        GraphScope::ExceedsCap
    );
}

#[test]
fn a_refused_graph_is_not_read_and_does_not_fail_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _, _) = store(dir.path());
    let absent = "https://example.org/project/absent";
    let measured = measure(&store, &request(&store, &[PROJECT, absent], None)).unwrap();
    assert_eq!(rows(&measured, PROJECT), 2);
    assert!(matches!(
        measured.graphs[absent],
        GraphScope::Refused { .. }
    ));
}

#[test]
fn a_measurement_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _, _) = store(dir.path());
    let writes = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&writes);
    store
        .conn
        .authorizer(Some(move |context: AuthContext<'_>| {
            if let AuthAction::Insert { table_name }
            | AuthAction::Delete { table_name }
            | AuthAction::Update { table_name, .. } = context.action
            {
                sink.lock().unwrap().push(table_name.to_string());
            }
            Authorization::Allow
        }))
        .unwrap();
    measure(&store, &request(&store, &[PROJECT, COMMENTS], None)).unwrap();
    store
        .conn
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    assert!(
        writes.lock().unwrap().is_empty(),
        "{:?}",
        writes.lock().unwrap()
    );
}

#[test]
fn malformed_or_unbudgeted_requests_refuse_whole() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _, _) = store(dir.path());
    assert!(measure(&store, &request(&store, &[], None)).is_err());
    assert!(measure(&store, &request(&store, &[PROJECT, PROJECT], None)).is_err());
    let many: Vec<String> = (0..17)
        .map(|i| format!("https://example.org/g/{i}"))
        .collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    assert!(measure(&store, &request(&store, &refs, None)).is_err());
    let mut unbudgeted = request(&store, &[PROJECT], None);
    unbudgeted.max_vm_steps = 0;
    assert!(measure(&store, &unbudgeted).is_err());
    let mut untimed = request(&store, &[PROJECT], None);
    untimed.deadline_ms = 0;
    assert!(measure(&store, &untimed).is_err());
    let mut lineage = request(&store, &[PROJECT], None);
    lineage.source_store_id = "urn:uuid:other".into();
    assert!(measure(&store, &lineage).is_err());
}

/// The store never writes a second visible copy of one `(e,a,v)`, but a merged
/// or imported file can hold one. It counts once, as both readers return it.
#[test]
fn a_physical_duplicate_counts_once_as_the_readers_return_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _, _) = store(dir.path());
    let g = store.registered_graph_id(PROJECT).unwrap().unwrap();
    let head = store.latest_tx_id().unwrap();
    let columns: Vec<String> = store
        .conn
        .prepare("SELECT name FROM pragma_table_info('facts')")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let copied: Vec<String> = columns
        .iter()
        .map(|c| {
            if c == "tx" {
                "?3".to_string()
            } else {
                c.clone()
            }
        })
        .collect();
    let physical = |store: &Store| -> i64 {
        store
            .conn
            .query_row(
                &format!("SELECT count(*) {}", main_graph::PHYSICAL_ROWS),
                rusqlite::params![g, head],
                |r| r.get(0),
            )
            .unwrap()
    };
    let before = physical(&store);
    let measured_before = rows(
        &measure(&store, &request(&store, &[PROJECT], None)).unwrap(),
        PROJECT,
    );
    // Copy one visible row under an earlier transaction id: same (e,a,v).
    let earliest: i64 = store
        .conn
        .query_row("SELECT MIN(id) FROM transactions", [], |r| r.get(0))
        .unwrap();
    let inserted = store
        .conn
        .execute(
            &format!(
                "INSERT INTO facts ({}) SELECT {} {} AND tx<>?3 LIMIT 1",
                columns.join(","),
                copied.join(","),
                main_graph::PHYSICAL_ROWS
            ),
            rusqlite::params![g, head, earliest],
        )
        .unwrap();
    assert_eq!(inserted, 1, "precondition: a duplicate was written");
    assert_eq!(
        physical(&store),
        before + 1,
        "precondition: it is visible at the head"
    );
    let measured = measure(&store, &request(&store, &[PROJECT], None)).unwrap();
    assert_eq!(rows(&measured, PROJECT), measured_before);
    let composed = super::super::composed::acquire(
        &store,
        &super::super::composed::Request {
            source_store_id: store.store_id().unwrap(),
            graphs: vec![PROJECT.into()],
            tx: None,
            expected_generation: None,
            max_bytes: None,
        },
    )
    .unwrap();
    assert_eq!(
        rows(&measured, PROJECT),
        composed.source_receipt.main.graphs[PROJECT].triples as u64
    );
}
