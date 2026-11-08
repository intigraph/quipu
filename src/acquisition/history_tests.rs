use super::*;
use crate::{Datum, Op};

const GRAPH: &str = "https://example.org/project/board";
const SUBJECT: &str = "https://example.org/item/one";

fn write(store: &mut Store, graph: Option<i64>, value: Value, op: Op) -> i64 {
    let datum = Datum {
        entity: store.intern(SUBJECT).unwrap(),
        attribute: store.intern("https://example.org/value").unwrap(),
        value,
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op,
    };
    store
        .transact_to_graph(
            &[datum],
            "2026-01-01T00:00:00Z",
            Some("actor"),
            Some("fixture"),
            graph.unwrap_or(crate::schema::ROOT_GRAPH),
        )
        .unwrap()
}

fn fixture() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let graph = store.graph_create(GRAPH).unwrap();
    write(
        &mut store,
        Some(graph),
        Value::Str("retained".into()),
        Op::Assert,
    );
    write(
        &mut store,
        Some(graph),
        Value::Str("retained".into()),
        Op::Retract,
    );
    store
}

fn input(store: &Store) -> JsonValue {
    json!({"iri":SUBJECT,"graph":GRAPH,"source_store_id":store.store_id().unwrap()})
}

fn read_json(store: &Store, input: &JsonValue) -> Result<JsonValue> {
    crate::tool_entity_history(store, input)
}

#[test]
fn qualified_history_preserves_named_graph_assert_retract_and_provenance() {
    let mut store = Store::open_in_memory().unwrap();
    let graph = store.graph_create(GRAPH).unwrap();
    let provenance = crate::write_provenance::RequestProvenance::classify(
        "agent-adhoc",
        "/knot",
        [
            Some("declared-actor"),
            Some("service"),
            Some("unit-host"),
            None,
            None,
        ],
    );
    crate::write_provenance::scoped(Some(std::sync::Arc::new(provenance)), || {
        write(
            &mut store,
            Some(graph),
            Value::Str("retained".into()),
            Op::Assert,
        );
    });
    write(
        &mut store,
        Some(graph),
        Value::Str("retained".into()),
        Op::Retract,
    );
    let before = read_json(&store, &json!({"iri":SUBJECT})).unwrap();
    assert_eq!(before["count"], 0); // Legacy ROOT scope cannot see this graph.
    let request = input(&store);
    let after = read_json(&store, &request).unwrap();
    assert_eq!(after["schema"], CONTRACT);
    assert_eq!(after["count"], 2);
    assert_eq!(after["history"][0]["op"], "assert");
    assert_eq!(after["history"][1]["op"], "retract");
    assert_eq!(after["history"][0]["declared"]["host"], "unit-host");
    assert!(after["history"][0]["authenticated"].is_null());
    assert_eq!(after["history"][0]["transaction"]["actor"], "actor");
    assert_eq!(after["source_receipt"]["continuous_history"], false);
    let qualified = &after["history"][0]["qualified_tx"];
    assert_eq!(qualified["source_store_id"], request["source_store_id"]);
    assert_eq!(
        qualified["generation"],
        after["source_receipt"]["generation"]
    );
    assert!(qualified["local_tx"].as_i64().unwrap() > 0);
    eprintln!(
        "DEMO_HISTORY {}",
        json!({"target":"isolated in-memory candidate", "request":request,"legacy_root_response":before,"qualified_response":after})
    );
}

#[test]
fn qualified_history_does_not_mix_root_or_other_graph_rows() {
    let mut store = fixture();
    write(&mut store, None, Value::Str("root-only".into()), Op::Assert);
    let other = store.graph_create("https://example.org/other").unwrap();
    write(
        &mut store,
        Some(other),
        Value::Str("other-only".into()),
        Op::Assert,
    );
    let result = read_json(&store, &input(&store)).unwrap();
    assert_eq!(result["count"], 2);
    for row in result["history"].as_array().unwrap() {
        assert_eq!(row["value"], json!({"type":"string","value":"retained"}));
    }
    assert_eq!(
        read_json(&store, &json!({"iri":SUBJECT})).unwrap()["count"],
        1
    );
}

#[test]
fn qualified_history_ranges_need_source_generation_and_refuse_future_or_wrong_identity() {
    let store = fixture();
    let request = input(&store);
    let first = read_json(&store, &request).unwrap();
    let generation = first["source_receipt"]["generation"].clone();
    let mut bounded = request.clone();
    bounded["until_tx"] = first["history"][0]["qualified_tx"]["local_tx"].clone();
    assert!(read_json(&store, &bounded).is_err());
    bounded["expected_generation"] = generation;
    assert_eq!(read_json(&store, &bounded).unwrap()["count"], 1);
    bounded["until_tx"] = json!(store.latest_tx_id().unwrap() + 1);
    assert!(read_json(&store, &bounded).is_err());
    bounded["until_tx"] = json!(-1);
    assert!(read_json(&store, &bounded).is_err());
    bounded["until_tx"] = json!(true);
    assert!(read_json(&store, &bounded).is_err());
    let mut wrong = request.clone();
    wrong["source_store_id"] = json!("wrong-source");
    assert!(read_json(&store, &wrong).is_err());
    wrong = request.clone();
    wrong["expected_generation"] = json!("stale-generation");
    assert!(read_json(&store, &wrong).is_err());
    // All refusal paths restore the budget/transaction; a fresh read still works.
    assert_eq!(read_json(&store, &request).unwrap()["count"], 2);
}

#[test]
fn qualified_history_caps_refuse_instead_of_returning_partial_success() {
    let store = fixture();
    for (field, cap) in [
        ("max_entries", 1),
        ("max_bytes", 32),
        ("max_entries", 0),
        ("max_entries", 10001),
    ] {
        let mut request = input(&store);
        request[field] = json!(cap);
        assert!(read_json(&store, &request).is_err(), "{field}={cap}");
    }
    assert_eq!(read_json(&store, &input(&store)).unwrap()["count"], 2);
}

#[test]
fn qualified_history_rejects_unknown_scopes_without_interning() {
    let store = fixture();
    let head = store.latest_tx_id().unwrap();
    let mut unknown = input(&store);
    unknown["graph"] = json!("https://example.org/missing");
    assert!(read_json(&store, &unknown).is_err());
    unknown = input(&store);
    unknown["iri"] = json!("https://example.org/missing-item");
    assert!(read_json(&store, &unknown).is_err());
    assert!(
        store
            .lookup("https://example.org/missing-item")
            .unwrap()
            .is_none()
    );
    unknown = input(&store);
    unknown["graph"] = JsonValue::Null;
    assert!(read_json(&store, &unknown).is_err());
    unknown = input(&store);
    unknown["provider"] = json!("archive");
    assert!(read_json(&store, &unknown).is_err());
    assert!(read_json(&store, &json!({"iri":SUBJECT,"tx":1})).is_err());
    assert!(read_json(&store, &json!({"iri":SUBJECT,"observation_only":true})).is_err());
    assert!(read_json(&store, &json!({"iri":SUBJECT,"max_entries":1})).is_err());
    assert_eq!(store.latest_tx_id().unwrap(), head);
}

#[test]
fn qualified_history_values_are_lossless_and_typed() {
    let mut store = Store::open_in_memory().unwrap();
    let graph = store.graph_create(GRAPH).unwrap();
    let target = store.intern("https://example.org/target").unwrap();
    let values = [
        Value::Int(i64::MAX),
        Value::Float(-0.0),
        Value::Bytes(vec![0, 255]),
        Value::Ref(target),
        Value::Lang {
            lexical: "hello@world".into(),
            lang: "en".into(),
        },
        Value::Typed {
            lexical: "001.00".into(),
            datatype: "http://www.w3.org/2001/XMLSchema#decimal".into(),
        },
    ];
    for value in values {
        write(&mut store, Some(graph), value, Op::Assert);
    }
    let result = read_json(&store, &input(&store)).unwrap();
    let entries: Vec<_> = result["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["value"].clone())
        .collect();
    for expected in [
        json!({"type":"integer","value":i64::MAX.to_string()}),
        json!({"type":"float64","bits":"8000000000000000"}),
        json!({"type":"bytes","encoding":"hex","value":"00ff"}),
        json!({"type":"iri","value":"https://example.org/target"}),
        json!({"type":"lang","value":"hello@world","lang":"en"}),
        json!({"type":"typed","value":"001.00","datatype":"http://www.w3.org/2001/XMLSchema#decimal"}),
    ] {
        assert!(
            entries.contains(&expected),
            "missing {expected}: {entries:?}"
        );
    }
}

#[test]
fn qualified_history_observation_bootstraps_ranges_without_claiming_history() {
    let store = fixture();
    let mut request = input(&store);
    request["max_entries"] = json!(1);
    assert!(read_json(&store, &request).is_err());
    request["observation_only"] = json!(true);
    let observed = read_json(&store, &request).unwrap();
    assert_eq!(observed["schema"], OBSERVATION_CONTRACT);
    for field in ["history", "count", "complete", "source_receipt"] {
        assert!(observed.get(field).is_none());
    }
    request["observation_only"] = json!(false);
    request["expected_generation"] = observed["generation"].clone();
    request["until_tx"] = json!(observed["observed_head_tx"].as_i64().unwrap() - 1);
    assert_eq!(read_json(&store, &request).unwrap()["count"], 1);
}

#[test]
fn qualified_history_reopen_invalidates_generation_even_with_same_lineage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    let graph = store.graph_create(GRAPH).unwrap();
    write(
        &mut store,
        Some(graph),
        Value::Str("retained".into()),
        Op::Assert,
    );
    let mut request = input(&store);
    let result = read_json(&store, &request).unwrap();
    request["expected_generation"] = result["source_receipt"]["generation"].clone();
    drop(store);
    let store = Store::open(path.to_str().unwrap()).unwrap();
    assert!(read_json(&store, &request).is_err());
    request
        .as_object_mut()
        .unwrap()
        .remove("expected_generation");
    assert_eq!(read_json(&store, &request).unwrap()["count"], 1);
}

#[test]
fn qualified_history_topology_aba_and_policy_changes_invalidate_generation() {
    let mut store = fixture();
    let mut request = input(&store);
    let result = read_json(&store, &request).unwrap();
    request["expected_generation"] = result["source_receipt"]["generation"].clone();
    store.invalidate_acquisition_topology();
    assert!(read_json(&store, &request).is_err());
    request
        .as_object_mut()
        .unwrap()
        .remove("expected_generation");
    let fresh = read_json(&store, &request).unwrap();
    request["expected_generation"] = fresh["source_receipt"]["generation"].clone();
    store
        .labels_config_mut()
        .deny_policy_tokens
        .push("restricted".into());
    assert!(
        read_json(&store, &request)
            .unwrap_err()
            .to_string()
            .contains("generation mismatch")
    );
    request
        .as_object_mut()
        .unwrap()
        .remove("expected_generation");
    assert_eq!(read_json(&store, &request).unwrap()["count"], 2);
}

#[test]
fn qualified_history_sql_budget_interrupts_and_is_cleared() {
    let store = fixture();
    let expired = crate::time::set_request_deadline(Some(crate::time::Deadline::after_millis(0)));
    let budget = Budget::install(&store).unwrap();
    let grind = "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000) SELECT sum(x) FROM n";
    assert!(
        store
            .conn
            .query_row(grind, [], |r| r.get::<_, i64>(0))
            .is_err()
    );
    drop(budget);
    drop(expired);
    let positive = "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000) SELECT sum(x) FROM n";
    assert_eq!(
        store
            .conn
            .query_row(positive, [], |r| r.get::<_, i64>(0))
            .unwrap(),
        500500
    );
    assert_eq!(read_json(&store, &input(&store)).unwrap()["count"], 2);
}

#[cfg(target_os = "linux")]
#[test]
fn qualified_history_keeps_frozen_providers_outside_its_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("main.db").to_str().unwrap()).unwrap();
    let graph = store.graph_create(GRAPH).unwrap();
    write(
        &mut store,
        Some(graph),
        Value::Str("main".into()),
        Op::Assert,
    );
    let mut old = input(&store);
    old["expected_generation"] =
        read_json(&store, &old).unwrap()["source_receipt"]["generation"].clone();
    let cold = "https://example.org/cold";
    let cold_id = store.graph_create(cold).unwrap();
    write(
        &mut store,
        Some(cold_id),
        Value::Str("cold".into()),
        Op::Assert,
    );
    store
        .freeze_graph(
            cold,
            dir.path().to_str().unwrap(),
            "2026-01-02T00:00:00Z",
            None,
        )
        .unwrap();
    assert!(read_json(&store, &old).is_err());
    let result = read_json(&store, &input(&store)).unwrap();
    assert_eq!(result["count"], 1);
    assert!(
        !result["source_receipt"]["excluded_attachment_aliases"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut frozen = input(&store);
    frozen["graph"] = json!(cold);
    assert!(read_json(&store, &frozen).is_err());
    let entity = store.lookup(SUBJECT).unwrap().unwrap();
    let conflicting = store
        .intern("https://example.org/conflicting-identity")
        .unwrap();
    store
        .conn
        .execute(
            "INSERT OR REPLACE INTO temp.term_alias(alias_id,canonical_id) VALUES (?1,?2)",
            params![entity, conflicting],
        )
        .unwrap();
    let refusal = read_json(&store, &input(&store)).unwrap_err().to_string();
    assert!(refusal.contains("composed alias"), "{refusal}");
}
