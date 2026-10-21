//! Cross-write upper bounds must be checked before the write savepoint commits.
#![cfg(feature = "shacl")]
use quipu::{Datum, Op, Store, Value};

#[path = "write_cardinality/deactivation.rs"]
mod deactivation;

const TS: &str = "2026-10-21T00:00:00Z";
const TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const SHAPES: &str = r#"
@prefix ex: <https://example.org/d6s/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
ex:S a sh:NodeShape; sh:targetClass ex:C;
 sh:property [sh:path ex:value; sh:maxCount 1].
"#;

fn datum(store: &Store, predicate: &str, value: Value, op: Op) -> Datum {
    Datum {
        entity: store.intern("https://example.org/d6s/item").unwrap(),
        attribute: store.intern(predicate).unwrap(),
        value,
        valid_from: TS.into(),
        valid_to: None,
        op,
    }
}
fn batch(store: &Store, value: &str, typed: bool) -> Vec<Datum> {
    let mut data = vec![datum(
        store,
        "https://example.org/d6s/value",
        Value::Str(value.into()),
        Op::Assert,
    )];
    if typed {
        data.push(datum(
            store,
            TYPE,
            Value::Ref(store.intern("https://example.org/d6s/C").unwrap()),
            Op::Assert,
        ));
    }
    data
}
fn write(store: &mut Store, data: &[Datum], graph: i64) -> quipu::Result<i64> {
    store.with_write_shapes(Some(SHAPES), |store| {
        store.transact_to_graph(data, TS, None, None, graph)
    })
}
fn values(store: &Store, graph: i64) -> Vec<Value> {
    let e = store
        .lookup("https://example.org/d6s/item")
        .unwrap()
        .unwrap();
    let a = store
        .lookup("https://example.org/d6s/value")
        .unwrap()
        .unwrap();
    store
        .entity_facts_in_graph(e, graph)
        .unwrap()
        .into_iter()
        .filter(|f| f.attribute == a)
        .map(|f| f.value)
        .collect()
}
#[test]
fn second_write_rejected_even_without_repeating_type_and_same_value_is_idempotent() {
    let mut store = Store::open_in_memory().unwrap();
    let first = batch(&store, "A", true);
    write(&mut store, &first, 0).unwrap();
    write(&mut store, &first, 0).unwrap();
    let second = batch(&store, "B", false);
    let result = write(&mut store, &second, 0);
    assert!(
        matches!(result, Err(quipu::Error::ValidationFailed { .. })),
        "{result:?}"
    );
    assert_eq!(values(&store, 0), vec![Value::Str("A".into())]);
}
#[test]
fn retract_and_assert_are_validated_as_one_poststate_and_graphs_are_isolated() {
    let mut store = Store::open_in_memory().unwrap();
    let a = batch(&store, "A", true);
    write(&mut store, &a, 0).unwrap();
    let g = store.graph_create("https://example.org/d6s/other").unwrap();
    let b = batch(&store, "B", true);
    write(&mut store, &b, g).unwrap();
    let mut replace = batch(&store, "B", false);
    replace.insert(
        0,
        datum(
            &store,
            "https://example.org/d6s/value",
            Value::Str("A".into()),
            Op::Retract,
        ),
    );
    write(&mut store, &replace, 0).unwrap();
    assert_eq!(values(&store, 0), vec![Value::Str("B".into())]);
    assert_eq!(values(&store, g), vec![Value::Str("B".into())]);
}
#[test]
fn explicit_shape_scope_does_not_leak_after_error_or_unwind() {
    let mut store = Store::open_in_memory().unwrap();
    let _: quipu::Result<()> =
        store.with_write_shapes(Some(SHAPES), |_| Err(quipu::Error::Store("abort".into())));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: quipu::Result<()> =
            store.with_write_shapes(Some(SHAPES), |_| panic!("scope unwind"));
    }));
    assert!(caught.is_err());
    let a = batch(&store, "A", true);
    store.transact(&a, TS, None, None).unwrap();
    let b = batch(&store, "B", true);
    store.transact(&b, TS, None, None).unwrap();
    assert_eq!(
        values(&store, 0).len(),
        2,
        "low-level unconfigured writes keep their existing contract"
    );
}

#[test]
fn loaded_shapes_are_enforced_on_configured_and_conditional_writes() {
    let mut store = Store::open_in_memory().unwrap();
    store.load_shapes("max", SHAPES, TS).unwrap();
    store.shacl_config_mut().validate_on_write = true;
    let a = batch(&store, "A", true);
    store.transact(&a, TS, None, None).unwrap();
    let b = batch(&store, "B", true);
    assert!(store.transact(&b, TS, None, None).is_err());
    store.shacl_config_mut().validate_on_write = false;
    let expected = quipu::store::conditional::ExpectedValue {
        entity: a[0].entity,
        attribute: a[0].attribute,
        value: Some(Value::Str("A".into())),
    };
    assert!(
        store
            .transact_to_graph_if(&b, TS, None, None, 0, &[expected])
            .is_err()
    );
    assert_eq!(values(&store, 0), vec![Value::Str("A".into())]);
}

#[test]
fn snapshot_replacement_preserves_other_producers_claims() {
    use quipu::mcp::knot::tool_knot;
    let mut store = Store::open_in_memory().unwrap();
    store.load_shapes("max", SHAPES, TS).unwrap();
    let request = |value: &str, source: &str, replace: bool| {
        serde_json::json!({
            "turtle": format!("<https://example.org/d6s/item> a <https://example.org/d6s/C>; <https://example.org/d6s/value> {value:?}."),
            "timestamp": TS, "replace_snapshot": replace, "snapshot": source, "source": source,
        })
    };
    tool_knot(&mut store, &request("A", "one", true)).unwrap();
    tool_knot(&mut store, &request("A", "other", false)).unwrap();
    assert!(
        tool_knot(&mut store, &request("B", "one", true)).is_err(),
        "other producer still owns A"
    );
    assert_eq!(values(&store, 0), vec![Value::Str("A".into())]);
    let mut fresh = Store::open_in_memory().unwrap();
    fresh.load_shapes("max", SHAPES, TS).unwrap();
    tool_knot(&mut fresh, &request("A", "one", true)).unwrap();
    tool_knot(&mut fresh, &request("B", "one", true)).unwrap();
    assert_eq!(values(&fresh, 0), vec![Value::Str("B".into())]);
    let e = fresh
        .lookup("https://example.org/d6s/item")
        .unwrap()
        .unwrap();
    assert!(
        fresh
            .entity_history(e)
            .unwrap()
            .iter()
            .any(|f| f.value == Value::Str("A".into())),
        "replaced value retained in history"
    );
}

#[test]
fn multi_graph_write_rolls_back_earlier_graph_when_a_later_graph_fails() {
    let mut store = Store::open_in_memory().unwrap();
    let g = store.graph_create("https://example.org/d6s/g").unwrap();
    let a = batch(&store, "A", true);
    write(&mut store, &a, g).unwrap();
    let b = batch(&store, "B", true);
    let result = store.with_write_shapes(Some(SHAPES), |store| {
        store.transact_graph_batches(&[(0, b.clone()), (g, b)], TS, None, None)
    });
    assert!(result.is_err());
    assert!(values(&store, 0).is_empty());
    assert_eq!(values(&store, g), vec![Value::Str("A".into())]);
}

#[test]
fn root_subclass_context_targets_named_graph_without_counting_root_values() {
    let mut store = Store::open_in_memory().unwrap();
    let class = store.intern("https://example.org/d6s/Sub").unwrap();
    let hierarchy = Datum {
        entity: class,
        attribute: store
            .intern("http://www.w3.org/2000/01/rdf-schema#subClassOf")
            .unwrap(),
        value: Value::Ref(store.intern("https://example.org/d6s/C").unwrap()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    store.transact(&[hierarchy], TS, None, None).unwrap();
    let mut a = batch(&store, "A", true);
    a[1].value = Value::Ref(class);
    store.transact(&a, TS, None, None).unwrap();
    let g = store.graph_create("https://example.org/d6s/g").unwrap();
    let b = batch(&store, "B", false);
    write(&mut store, &b, g).unwrap();
    let c = batch(&store, "C", false);
    assert!(write(&mut store, &c, g).is_err());
    assert_eq!(values(&store, g), vec![Value::Str("B".into())]);
}

#[test]
fn unsupported_complex_upper_bound_fails_closed_instead_of_partial_validation() {
    let mut store = Store::open_in_memory().unwrap();
    let complex = SHAPES.replace("sh:path ex:value", "sh:path (ex:first ex:value)");
    let a = batch(&store, "A", true);
    let result = store.with_write_shapes(Some(&complex), |s| s.transact(&a, TS, None, None));
    assert!(
        matches!(result, Err(quipu::Error::CannotVerify(_))),
        "{result:?}"
    );
    assert!(values(&store, 0).is_empty());
}

#[test]
fn concurrent_writers_cannot_both_commit_distinct_values() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("concurrent.db");
    let mut store = Store::open(db.to_str().unwrap()).unwrap();
    let type_only = vec![datum(
        &store,
        TYPE,
        Value::Ref(store.intern("https://example.org/d6s/C").unwrap()),
        Op::Assert,
    )];
    store.transact(&type_only, TS, None, None).unwrap();
    let first = Store::open(db.to_str().unwrap()).unwrap();
    let second = Store::open(db.to_str().unwrap()).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = [first, second]
        .into_iter()
        .enumerate()
        .map(|(i, mut store)| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let data = batch(&store, &format!("value{i}"), false);
                barrier.wait();
                write(&mut store, &data, 0)
            })
        })
        .collect();
    let outcomes: Vec<_> = handles.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(
        outcomes.iter().filter(|r| r.is_ok()).count(),
        1,
        "{outcomes:?}"
    );
    assert_eq!(values(&store, 0).len(), 1);
}

#[test]
fn object_target_is_checked_when_an_edge_makes_it_applicable() {
    let mut store = Store::open_in_memory().unwrap();
    let two_values = [batch(&store, "A", false), batch(&store, "B", false)].concat();
    store.transact(&two_values, TS, None, None).unwrap();
    let shapes = SHAPES.replace("sh:targetClass ex:C", "sh:targetObjectsOf ex:link");
    let link = Datum {
        entity: store.intern("https://example.org/d6s/parent").unwrap(),
        attribute: store.intern("https://example.org/d6s/link").unwrap(),
        value: Value::Ref(two_values[0].entity),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    assert!(
        store
            .with_write_shapes(Some(&shapes), |s| s.transact(&[link], TS, None, None))
            .is_err()
    );
}

#[test]
fn language_literals_are_distinct_but_identical_terms_are_not_counted_twice() {
    let mut store = Store::open_in_memory().unwrap();
    let mut a = batch(&store, "A", true);
    a[0].value = Value::Lang {
        lexical: "word".into(),
        lang: "en".into(),
    };
    write(&mut store, &a, 0).unwrap();
    write(&mut store, &a, 0).unwrap();
    let mut b = a.clone();
    b[0].value = Value::Lang {
        lexical: "word".into(),
        lang: "fr".into(),
    };
    assert!(write(&mut store, &b, 0).is_err());
    assert_eq!(values(&store, 0), vec![a[0].value.clone()]);
}

#[test]
fn rejected_episode_update_keeps_its_prior_activity_metadata() {
    use quipu::episode::{Episode, ingest_episode};
    const NS: &str = "https://example.org/d6s/";
    let mut store = Store::open_in_memory().unwrap();
    let episode = |target: &str, body: &str| -> Episode {
        serde_json::from_value(serde_json::json!({
        "name": "activity", "episode_body": body, "source": "fixture",
        "nodes": [{"name": "item", "type": "C"}, {"name": target, "type": "Target"}],
        "edges": [{"source":"item", "target":target, "relation":"<https://example.org/d6s/value>"}],
        "shapes": SHAPES,
    })).unwrap()
    };
    ingest_episode(&mut store, &episode("first", "original"), TS, NS).unwrap();
    let ep = store
        .lookup(&format!("{NS}episode_activity"))
        .unwrap()
        .unwrap();
    let before = store.entity_facts(ep).unwrap();
    assert!(
        ingest_episode(
            &mut store,
            &episode("second", "changed"),
            "2026-10-21T00:01:00Z",
            NS
        )
        .is_err()
    );
    let after = store.entity_facts(ep).unwrap();
    let keys = |facts: Vec<quipu::Fact>| -> std::collections::BTreeSet<_> {
        facts
            .into_iter()
            .map(|f| (f.attribute, f.value.term_key()))
            .collect()
    };
    assert_eq!(
        keys(before),
        keys(after),
        "failed payload must not erase its earlier activity"
    );
}

#[test]
fn object_target_uses_existing_incoming_edge_when_only_the_focus_changes() {
    let mut store = Store::open_in_memory().unwrap();
    let a = batch(&store, "A", false);
    let link = Datum {
        entity: store.intern("https://example.org/d6s/parent").unwrap(),
        attribute: store.intern("https://example.org/d6s/link").unwrap(),
        value: Value::Ref(a[0].entity),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    store
        .transact(&[a, vec![link]].concat(), TS, None, None)
        .unwrap();
    let shapes = SHAPES.replace("sh:targetClass ex:C", "sh:targetObjectsOf ex:link");
    let b = batch(&store, "B", false);
    assert!(
        store
            .with_write_shapes(Some(&shapes), |s| s.transact(&b, TS, None, None))
            .is_err()
    );
    assert_eq!(values(&store, 0), vec![Value::Str("A".into())]);
}

#[test]
fn stored_blank_node_identity_survives_context_serialization() {
    let mut store = Store::open_in_memory().unwrap();
    let blank = store.intern("_:stableFocus").unwrap();
    let mut a = batch(&store, "A", true);
    for d in &mut a {
        d.entity = blank;
    }
    write(&mut store, &a, 0).unwrap();
    let mut b = batch(&store, "B", false);
    b[0].entity = blank;
    assert!(matches!(
        write(&mut store, &b, 0),
        Err(quipu::Error::ValidationFailed { .. })
    ));
    let value = store
        .lookup("https://example.org/d6s/value")
        .unwrap()
        .unwrap();
    assert_eq!(
        store
            .entity_facts(blank)
            .unwrap()
            .iter()
            .filter(|f| f.attribute == value)
            .count(),
        1
    );
}

#[test]
fn inverse_path_counts_prior_incoming_edges_for_the_same_focus() {
    let mut store = Store::open_in_memory().unwrap();
    let shapes = SHAPES
        .replace("sh:targetClass ex:C", "sh:targetObjectsOf ex:link")
        .replace("sh:path ex:value", "sh:path [sh:inversePath ex:link]");
    let link = |store: &Store, name: &str| Datum {
        entity: store
            .intern(&format!("https://example.org/d6s/{name}"))
            .unwrap(),
        attribute: store.intern("https://example.org/d6s/link").unwrap(),
        value: Value::Ref(store.intern("https://example.org/d6s/focus").unwrap()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    let first = link(&store, "first");
    store
        .with_write_shapes(Some(&shapes), |s| s.transact(&[first], TS, None, None))
        .unwrap();
    let second = link(&store, "second");
    assert!(matches!(
        store.with_write_shapes(Some(&shapes), |s| s.transact(&[second], TS, None, None)),
        Err(quipu::Error::ValidationFailed { .. })
    ));
}

#[test]
fn inverse_path_also_checks_literal_object_targets() {
    let mut store = Store::open_in_memory().unwrap();
    let shapes = SHAPES
        .replace("sh:targetClass ex:C", "sh:targetObjectsOf ex:link")
        .replace("sh:path ex:value", "sh:path [sh:inversePath ex:link]");
    let link = |store: &Store, name: &str| Datum {
        entity: store
            .intern(&format!("https://example.org/d6s/{name}"))
            .unwrap(),
        attribute: store.intern("https://example.org/d6s/link").unwrap(),
        value: Value::Str("literal focus".into()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    let first = link(&store, "first");
    store
        .with_write_shapes(Some(&shapes), |s| s.transact(&[first], TS, None, None))
        .unwrap();
    let second = link(&store, "second");
    assert!(matches!(
        store.with_write_shapes(Some(&shapes), |s| s.transact(&[second], TS, None, None)),
        Err(quipu::Error::ValidationFailed { .. })
    ));
}

#[test]
fn relative_manifest_iris_in_min_count_and_deactivated_shapes_match_batch() {
    for (shapes, subject) in [
        (
            include_str!("fixtures/write_relative_shapes/minCount-002.ttl"),
            "http://datashapes.org/sh/tests/core/property/minCount-002.test#ValidResource1",
        ),
        (
            include_str!("fixtures/write_relative_shapes/deactivated-001.ttl"),
            "http://datashapes.org/sh/tests/core/misc/deactivated-001.test#InvalidResource",
        ),
    ] {
        let resource = "http://www.w3.org/2000/01/rdf-schema#Resource";
        let data = format!("<{subject}> <{TYPE}> <{resource}> .");
        assert!(quipu::validate_shapes(shapes, &data).unwrap().conforms);
        let mut store = Store::open_in_memory().unwrap();
        let datum = Datum {
            entity: store.intern(subject).unwrap(),
            attribute: store.intern(TYPE).unwrap(),
            value: Value::Ref(store.intern(resource).unwrap()),
            valid_from: TS.into(),
            valid_to: None,
            op: Op::Assert,
        };
        store
            .with_write_shapes(Some(shapes), |store| {
                store.transact(&[datum], TS, None, None)
            })
            .unwrap();
        assert!(
            !store
                .entity_facts(store.lookup(subject).unwrap().unwrap())
                .unwrap()
                .is_empty()
        );
    }
}
