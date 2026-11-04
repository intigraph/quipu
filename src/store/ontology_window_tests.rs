use super::*;
use crate::mcp::owl::tool_load_ontology;

const TS: &str = "2026-11-04T00:00:00Z";

fn fixture(n: usize) -> (Store, i64, i64, i64) {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_ontology(
            "selected",
            "<urn:A> <http://www.w3.org/2000/01/rdf-schema#subClassOf> <urn:B> .",
            TS,
        )
        .unwrap();
    let graph = store.ensure_companion_inferred_graph(0, TS).unwrap();
    let before = store.latest_tx_id().unwrap();
    let attribute = store.intern("urn:value").unwrap();
    let datums: Vec<_> = (0..n)
        .map(|i| Datum {
            entity: store.intern(&format!("urn:subject:{i}")).unwrap(),
            attribute,
            value: Value::Int(i as i64),
            valid_from: TS.into(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    let through = store
        .transact_to_graph(&datums, TS, None, Some(SOURCE), graph)
        .unwrap();
    (store, before, through, graph)
}

fn call(store: &mut Store, before: i64, through: i64, apply: bool, expect: Json) -> Result<Json> {
    tool_load_ontology(
        store,
        &json!({"action":"retract_materialization", "name":"selected", "after_tx":before, "through_tx":through, "apply":apply, "expect":expect}),
    )
}

#[test]
fn ontology_window_dispatch_is_bounded_resumable_and_keeps_registry_explicit() {
    let (mut store, before, through, graph) = fixture(10_001);
    let head = store.latest_tx_id().unwrap();
    let preview = call(&mut store, before, through, false, Json::Null).unwrap();
    assert_eq!(preview["planned"], 10_001);
    assert_eq!(preview["batch_statements"], 5_000);
    assert_eq!(store.latest_tx_id().unwrap(), head);
    assert!(call(&mut store, before, through, true, json!("wrong")).is_err());
    assert_eq!(store.latest_tx_id().unwrap(), head);
    let mut token = preview["expect"].clone();
    for remaining in [5_001, 1, 0] {
        let response = call(&mut store, before, through, true, token).unwrap();
        assert_eq!(response["remaining"], remaining);
        assert_eq!(response["ontology_removed"], false);
        token = response["expect"].clone();
    }
    assert_eq!(store.list_ontologies().unwrap().len(), 1);
    let assertions: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM facts WHERE g=?1 AND tx=?2 AND op=1",
            params![graph, through],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(assertions, 10_001, "history remains append-only");
    assert!(tool_load_ontology(&mut store, &json!({"action":"remove", "name":"selected"})).is_ok());
    assert!(store.list_ontologies().unwrap().is_empty());
}

#[test]
fn ontology_window_token_binds_registry_version_and_transaction_head() {
    let (mut store, before, through, _) = fixture(1);
    let preview = call(&mut store, before, through, false, Json::Null).unwrap();
    store
        .load_ontology(
            "selected",
            "<urn:C> <http://www.w3.org/2000/01/rdf-schema#subClassOf> <urn:D> .",
            "2026-11-04T01:00:00Z",
        )
        .unwrap();
    assert!(call(&mut store, before, through, true, preview["expect"].clone()).is_err());
    let preview = call(&mut store, before, through, false, Json::Null).unwrap();
    store.transact(&[], TS, None, Some("other-writer")).unwrap();
    assert!(call(&mut store, before, through, true, preview["expect"].clone()).is_err());
}

#[test]
fn ontology_window_guard_checks_registry_even_without_a_fact_head_change() {
    let (mut store, before, through, _) = fixture(1);
    let plan = store
        .ontology_window_plan(
            "selected",
            ClaimWindow {
                after: before,
                through,
            },
        )
        .unwrap();
    store.remove_ontology("selected").unwrap();
    assert!(store.check_window_guard(&plan.guard).is_err());
}

#[test]
fn ontology_window_token_cannot_be_replayed_on_another_store() {
    let (mut first, before, through, _) = fixture(1);
    let preview = call(&mut first, before, through, false, Json::Null).unwrap();
    let (mut second, other_before, other_through, _) = fixture(1);
    let head = second.latest_tx_id().unwrap();
    assert!(
        call(
            &mut second,
            other_before,
            other_through,
            true,
            preview["expect"].clone()
        )
        .is_err()
    );
    assert_eq!(second.latest_tx_id().unwrap(), head);
}

#[test]
fn ontology_window_dispatch_requires_strict_apply_and_explicit_bounds() {
    let (mut store, before, through, _) = fixture(1);
    let head = store.latest_tx_id().unwrap();
    for payload in [
        json!({"action":"retract_materialization", "name":"selected", "after_tx":before, "through_tx":through, "apply":true}),
        json!({"action":"retract_materialization", "name":"selected", "after_tx":before, "through_tx":through, "apply":"true"}),
        json!({"action":"retract_materialization", "name":"selected", "through_tx":through}),
        json!({"action":"retract_materialization", "name":"selected", "after_tx":before, "through_tx":through+1}),
    ] {
        assert!(tool_load_ontology(&mut store, &payload).is_err());
        assert_eq!(store.latest_tx_id().unwrap(), head);
    }
}
