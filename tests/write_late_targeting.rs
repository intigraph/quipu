//! Late target admission must judge stored values inside the write savepoint.
#![cfg(feature = "shacl")]
use quipu::{Datum, Error, Op, Store, Value};
const TS: &str = "2026-10-21T00:00:00Z";
const TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const EX: &str = "https://example.org/late/";
fn datum(store: &Store, subject: &str, predicate: &str, value: Value) -> Datum {
    Datum {
        entity: store.intern(subject).unwrap(),
        attribute: store.intern(predicate).unwrap(),
        value,
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}
fn write(store: &mut Store, shapes: &str, datums: &[Datum], graph: i64) -> quipu::Result<i64> {
    store.with_write_shapes(Some(shapes), |store| {
        store.transact_to_graph(datums, TS, None, None, graph)
    })
}
fn shape(component: &str) -> String {
    format!(
        r#"@prefix ex: <{EX}> . @prefix sh: <http://www.w3.org/ns/shacl#> .
    @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
    ex:S a sh:NodeShape; sh:targetClass ex:C; sh:property [sh:path ex:value; {component}]."#
    )
}
#[test]
fn bad_values_written_before_type_are_rejected_atomically() {
    let cases = [
        ("sh:datatype xsd:integer", Value::Str("bad".into())),
        (r#"sh:pattern "^ok$""#, Value::Str("bad".into())),
        (
            r#"sh:pattern "^ok$"; sh:flags "i""#,
            Value::Str("bad".into()),
        ),
        ("sh:minLength 3", Value::Str("x".into())),
        ("sh:maxLength 1", Value::Str("long".into())),
        ("sh:nodeKind sh:IRI", Value::Str("literal".into())),
        (r#"sh:in ("allowed")"#, Value::Str("other".into())),
        ("sh:minExclusive 1", Value::Int(1)),
        ("sh:minExclusive 0", Value::Int(-1)),
    ];
    for (constraint, value) in cases {
        let mut store = Store::open_in_memory().unwrap();
        let shapes = shape(constraint);
        let first = datum(
            &store,
            &(EX.to_owned() + "item"),
            &(EX.to_owned() + "value"),
            value,
        );
        write(&mut store, &shapes, &[first], 0).unwrap();
        let class = store.intern(&(EX.to_owned() + "C")).unwrap();
        let typed = datum(&store, &(EX.to_owned() + "item"), TYPE, Value::Ref(class));
        let result = write(&mut store, &shapes, &[typed], 0);
        assert!(
            matches!(result, Err(Error::ValidationFailed { .. })),
            "{constraint}: {result:?}"
        );
        let facts = store
            .entity_facts(store.lookup(&(EX.to_owned() + "item")).unwrap().unwrap())
            .unwrap();
        assert_eq!(
            facts.len(),
            1,
            "rejected type must roll back; prior value survives"
        );
    }
}
#[test]
fn valid_late_values_pass_and_an_unrelated_named_graph_cannot_repair_them() {
    for valid in [false, true] {
        let mut store = Store::open_in_memory().unwrap();
        let shapes = shape(r#"sh:pattern "^ok$""#);
        let other = store.graph_create("https://example.org/other").unwrap();
        let good = datum(
            &store,
            &(EX.to_owned() + "item"),
            &(EX.to_owned() + "value"),
            Value::Str("ok".into()),
        );
        write(&mut store, &shapes, &[good], other).unwrap();
        let value = datum(
            &store,
            &(EX.to_owned() + "item"),
            &(EX.to_owned() + "value"),
            Value::Str(if valid { "ok" } else { "bad" }.into()),
        );
        write(&mut store, &shapes, &[value], 0).unwrap();
        let class = store.intern(&(EX.to_owned() + "C")).unwrap();
        let typed = datum(&store, &(EX.to_owned() + "item"), TYPE, Value::Ref(class));
        let result = write(&mut store, &shapes, &[typed], 0);
        assert_eq!(result.is_ok(), valid, "{result:?}");
    }
}
#[test]
fn late_node_constraint_reads_the_existing_referenced_description() {
    for valid in [false, true] {
        let mut store = Store::open_in_memory().unwrap();
        let shapes =
            shape("sh:node ex:Child") + " ex:Child sh:property [sh:path ex:name; sh:minLength 3].";
        let child = store.intern(&(EX.to_owned() + "child")).unwrap();
        let description = datum(
            &store,
            &(EX.to_owned() + "child"),
            &(EX.to_owned() + "name"),
            Value::Str(if valid { "long" } else { "x" }.into()),
        );
        write(&mut store, &shapes, &[description], 0).unwrap();
        let value = datum(
            &store,
            &(EX.to_owned() + "item"),
            &(EX.to_owned() + "value"),
            Value::Ref(child),
        );
        write(&mut store, &shapes, &[value], 0).unwrap();
        let class = store.intern(&(EX.to_owned() + "C")).unwrap();
        let typed = datum(&store, &(EX.to_owned() + "item"), TYPE, Value::Ref(class));
        let result = write(&mut store, &shapes, &[typed], 0);
        assert_eq!(result.is_ok(), valid, "{result:?}");
    }
}

#[test]
fn late_target_subjects_of_checks_preexisting_value() {
    let mut store = Store::open_in_memory().unwrap();
    let shapes = shape(r#"sh:pattern "^ok$""#)
        .replace("sh:targetClass ex:C", "sh:targetSubjectsOf ex:activate");
    let value = datum(
        &store,
        &(EX.to_owned() + "item"),
        &(EX.to_owned() + "value"),
        Value::Str("bad".into()),
    );
    write(&mut store, &shapes, &[value], 0).unwrap();
    let trigger = datum(
        &store,
        &(EX.to_owned() + "item"),
        &(EX.to_owned() + "activate"),
        Value::Bool(true),
    );
    assert!(matches!(
        write(&mut store, &shapes, &[trigger], 0),
        Err(Error::ValidationFailed { .. })
    ));
}
#[test]
fn referenced_context_does_not_become_an_unrelated_write_owned_focus() {
    let mut store = Store::open_in_memory().unwrap();
    let class = store.intern(&(EX.to_owned() + "C")).unwrap();
    let child = store.intern(&(EX.to_owned() + "child")).unwrap();
    let bad = [
        datum(&store, &(EX.to_owned() + "child"), TYPE, Value::Ref(class)),
        datum(
            &store,
            &(EX.to_owned() + "child"),
            &(EX.to_owned() + "value"),
            Value::Str("legacy bad".into()),
        ),
    ];
    store.transact(&bad, TS, None, None).unwrap();
    let shapes = shape("sh:nodeKind sh:IRI");
    let value = datum(
        &store,
        &(EX.to_owned() + "item"),
        &(EX.to_owned() + "value"),
        Value::Ref(child),
    );
    write(&mut store, &shapes, &[value], 0).unwrap();
    let typed = datum(&store, &(EX.to_owned() + "item"), TYPE, Value::Ref(class));
    write(&mut store, &shapes, &[typed], 0).unwrap();
}

#[test]
fn late_object_target_is_owned_but_an_ordinary_reference_is_not() {
    for valid in [false, true] {
        let mut store = Store::open_in_memory().unwrap();
        let shapes = shape(r#"sh:pattern "^ok$""#)
            .replace("sh:targetClass ex:C", "sh:targetObjectsOf ex:activate");
        let item = store.intern(&(EX.to_owned() + "item")).unwrap();
        let value = datum(
            &store,
            &(EX.to_owned() + "item"),
            &(EX.to_owned() + "value"),
            Value::Str(if valid { "ok" } else { "bad" }.into()),
        );
        write(&mut store, &shapes, &[value], 0).unwrap();
        let trigger = datum(
            &store,
            &(EX.to_owned() + "trigger"),
            &(EX.to_owned() + "activate"),
            Value::Ref(item),
        );
        let result = write(&mut store, &shapes, &[trigger], 0);
        assert_eq!(result.is_ok(), valid, "{result:?}");
    }
}

#[test]
fn changing_a_referenced_node_rechecks_only_its_declared_node_dependents() {
    let mut store = Store::open_in_memory().unwrap();
    let shapes = shape("sh:node ex:Child")
        + " ex:Child sh:property [sh:path ex:name; sh:datatype <http://www.w3.org/2001/XMLSchema#string>].";
    let child = store.intern(&(EX.to_owned() + "child")).unwrap();
    let class = store.intern(&(EX.to_owned() + "C")).unwrap();
    let parent = [
        datum(&store, &(EX.to_owned() + "item"), TYPE, Value::Ref(class)),
        datum(
            &store,
            &(EX.to_owned() + "item"),
            &(EX.to_owned() + "value"),
            Value::Ref(child),
        ),
    ];
    write(&mut store, &shapes, &parent, 0).unwrap();
    let good = datum(
        &store,
        &(EX.to_owned() + "child"),
        &(EX.to_owned() + "name"),
        Value::Str("valid".into()),
    );
    write(&mut store, &shapes, &[good], 0).unwrap();
    let bad = datum(
        &store,
        &(EX.to_owned() + "child"),
        &(EX.to_owned() + "name"),
        Value::Int(123),
    );
    assert!(matches!(
        write(&mut store, &shapes, &[bad], 0),
        Err(Error::ValidationFailed { .. })
    ));
    let child_facts = store.entity_facts(child).unwrap();
    assert_eq!(
        child_facts.len(),
        1,
        "invalid child mutation must roll back"
    );
}
