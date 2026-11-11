use super::*;
use crate::store::Datum;
use crate::types::{Op, Value};
use serde_json::json;

const ROUTED: &str = "https://example.test/graphs/ingest";
const OTHER: &str = "https://example.test/graphs/unrouted";
const LATE: &str = "https://example.test/graphs/late";
const TS: &str = "2026-11-11T00:00:00Z";

fn legacy() -> (tempfile::TempDir, String, Store) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db").to_str().unwrap().to_string();
    let store = Store::open(&path).unwrap();
    (dir, path, store)
}

fn datum(store: &Store, tag: &str) -> Datum {
    Datum {
        entity: store.intern(&format!("urn:fence:{tag}")).unwrap(),
        attribute: store.intern("urn:fence:p").unwrap(),
        value: Value::Int(1),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

fn knot(store: &mut Store, graph: &str, tag: &str) -> Result<serde_json::Value> {
    let mut input = json!({
        "turtle": format!("<urn:fence:{tag}> <urn:fence:p> \"v\" ."),
        "source": "fence-test",
    });
    // ROOT is the default dataset: it is selected by omitting `graph`.
    if graph != crate::schema::ROOT_GRAPH_IRI {
        input["graph"] = json!(graph);
    }
    crate::tool_knot(store, &input)
}

fn count(store: &Store, sql: &str) -> i64 {
    store.conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn assert_fenced<T: std::fmt::Debug>(result: Result<T>, why: &str) {
    let err = result.expect_err(why).to_string();
    assert!(err.contains("routed to its own owner"), "{why}: {err}");
}

#[test]
fn inventory_classifies_every_graph_bearing_column() {
    let (_dir, _path, store) = legacy();
    let inventory = store.routed_fence_inventory().unwrap();
    let found: Vec<(&str, &str, FenceClass)> = inventory
        .iter()
        .map(|c| (c.table.as_str(), c.column.as_str(), c.class))
        .collect();
    // Discovered from a freshly migrated schema, not from CLASSES: a new
    // graph-bearing table changes this list or refuses the inventory.
    assert_eq!(
        found,
        vec![
            ("dataset_members", "graph_iri", FenceClass::Reference),
            ("denial_quarantine", "graph", FenceClass::WritesOnly),
            ("facts", "g", FenceClass::All),
            ("forks", "g", FenceClass::All),
            ("frozen_packs", "graph_iri", FenceClass::All),
            ("graph_consumers", "graph", FenceClass::WritesOnly),
            ("graph_events", "graph", FenceClass::WritesOnly),
            ("graphs", "g", FenceClass::All),
            ("pendant_repo_graphs", "graph", FenceClass::All),
            ("snapshot_uploads", "graph", FenceClass::WritesOnly),
            ("writer_graph_instances", "g", FenceClass::All),
        ]
    );
}

#[test]
fn unclassified_graph_column_refuses_install() {
    let (_dir, _path, store) = legacy();
    store
        .conn
        .execute_batch("CREATE TABLE new_feature (id INTEGER PRIMARY KEY, target_graph TEXT)")
        .unwrap();
    let err = store
        .fence_routed_graph(ROUTED, 7, 4)
        .unwrap_err()
        .to_string();
    assert!(err.contains("new_feature.target_graph"), "{err}");
    assert!(
        store.routed_graphs_fenced().unwrap().is_empty(),
        "nothing recorded"
    );
}

#[test]
fn fenced_graph_refuses_every_legacy_write_path_and_changes_nothing() {
    let (_dir, path, mut store) = legacy();
    knot(&mut store, crate::schema::ROOT_GRAPH_IRI, "root-before").unwrap();
    // ROUTED was registered while legacy owned it; LATE is not even interned
    // yet, so the fence must resolve it at write time.
    store.graph_create(ROUTED).unwrap();
    store.fence_routed_graph(ROUTED, 7, 4).unwrap();
    store.fence_routed_graph(LATE, 7, 4).unwrap();
    let facts = count(&store, "SELECT count(*) FROM facts");
    let graphs = count(&store, "SELECT count(*) FROM graphs");
    let txs = count(&store, "SELECT count(*) FROM transactions");

    assert_fenced(
        store.graph_create(LATE),
        "registering a late-interned graph",
    );
    assert_fenced(knot(&mut store, ROUTED, "knot"), "guarded knot");
    let routed = store.intern(ROUTED).unwrap();
    let batch = vec![
        (0, vec![datum(&store, "root-in-batch")]),
        (routed, vec![datum(&store, "routed")]),
    ];
    assert_fenced(
        store.transact_graph_batches(&batch, TS, None, None),
        "multi-graph batch",
    );
    // A different process or client obeys the same file-resident fence.
    let other = rusqlite::Connection::open(&path).unwrap();
    assert_fenced(
        other
            .execute(
                "INSERT INTO graph_events(graph, type, ts, tx_id) VALUES (?1, 'x', ?2, 1)",
                [ROUTED, TS],
            )
            .map_err(Error::from),
        "raw SQL from another connection",
    );

    assert_eq!(
        count(&store, "SELECT count(*) FROM facts"),
        facts,
        "no fact staged"
    );
    assert_eq!(
        count(&store, "SELECT count(*) FROM graphs"),
        graphs,
        "no graph row"
    );
    assert_eq!(count(&store, "SELECT count(*) FROM graph_events"), 0);
    assert_eq!(
        count(&store, "SELECT count(*) FROM transactions"),
        txs,
        "no orphan tx"
    );

    // Positive controls: ROOT and an unrouted graph still write.
    knot(&mut store, crate::schema::ROOT_GRAPH_IRI, "root-after").unwrap();
    store.graph_create(OTHER).unwrap();
    knot(&mut store, OTHER, "other").unwrap();
}

#[test]
fn retained_copy_is_frozen_but_retention_still_runs() {
    let (_dir, path, mut store) = legacy();
    store.graph_create(ROUTED).unwrap();
    knot(&mut store, ROUTED, "retained").unwrap();
    let routed = store.lookup(ROUTED).unwrap().unwrap();
    store
        .conn
        .execute(
            "INSERT INTO graph_events(graph, type, ts, tx_id) VALUES (?1, 'x', ?2, 1)",
            [ROUTED, TS],
        )
        .unwrap();
    store.fence_routed_graph(ROUTED, 7, 4).unwrap();
    let facts = count(&store, "SELECT count(*) FROM facts");
    assert!(facts > 0);

    let other = rusqlite::Connection::open(&path).unwrap();
    assert_fenced(
        other
            .execute("DELETE FROM facts WHERE g = ?1", [routed])
            .map_err(Error::from),
        "deleting retained facts",
    );
    assert_fenced(
        other
            .execute("UPDATE graphs SET class = 'overlay' WHERE g = ?1", [routed])
            .map_err(Error::from),
        "changing the retained graph",
    );
    assert_eq!(count(&store, "SELECT count(*) FROM facts"), facts);
    // Event retention may still delete old rows for the routed graph.
    other
        .execute("DELETE FROM graph_events WHERE graph = ?1", [ROUTED])
        .unwrap();
    assert_eq!(count(&store, "SELECT count(*) FROM graph_events"), 0);
    // Reads of the retained copy are unaffected.
    assert!(
        count(
            &store,
            &format!("SELECT count(*) FROM facts WHERE g = {routed}")
        ) > 0
    );
}

#[test]
fn verify_refuses_a_missing_trigger_or_a_later_unclassified_table() {
    let (_dir, _path, store) = legacy();
    store.fence_routed_graph(ROUTED, 7, 4).unwrap();
    store.verify_routed_fence().unwrap();
    // An on-demand derived table is classified and needs no trigger.
    store
        .conn
        .execute_batch(
            "CREATE TABLE qp_proj_example (g INTEGER NOT NULL, e INTEGER NOT NULL, key BLOB)",
        )
        .unwrap();
    store.verify_routed_fence().unwrap();

    store
        .conn
        .execute_batch("DROP TRIGGER quipu_routed_fence_facts_g_delete")
        .unwrap();
    let err = store.verify_routed_fence().unwrap_err().to_string();
    assert!(err.contains("facts.g (delete)"), "{err}");
    // Re-installing restores it.
    store.fence_routed_graph(ROUTED, 7, 4).unwrap();
    store.verify_routed_fence().unwrap();

    store
        .conn
        .execute_batch("CREATE TABLE later_feature (graph TEXT)")
        .unwrap();
    let err = store.verify_routed_fence().unwrap_err().to_string();
    assert!(err.contains("later_feature.graph"), "{err}");
}

#[test]
fn fence_identity_is_explicit() {
    let (_dir, _path, store) = legacy();
    for (graph, revision, generation) in [
        (crate::schema::ROOT_GRAPH_IRI, 7, 4),
        ("", 7, 4),
        (ROUTED, 0, 4),
        (ROUTED, 7, 0),
    ] {
        assert!(
            store
                .fence_routed_graph(graph, revision, generation)
                .is_err()
        );
    }
    store.fence_routed_graph(ROUTED, 7, 4).unwrap();
    store
        .fence_routed_graph(ROUTED, 7, 4)
        .expect("same fence is idempotent");
    assert!(
        store.fence_routed_graph(ROUTED, 8, 5).is_err(),
        "silent re-pin"
    );
    assert_eq!(
        store.routed_graphs_fenced().unwrap(),
        vec![ROUTED.to_string()]
    );
}
