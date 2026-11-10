//! Split writes must not hide value-local logical or upper-bound failures.
#![cfg(feature = "shacl")]

use quipu::{Datum, Error, Op, Store, Value};

const EX: &str = "https://example.org/local/";
const TS: &str = "2026-01-01T00:00:00Z";

#[test]
fn nonlocal_not_keeps_existing_policy_beside_a_value_local_not() {
    let mut store = Store::open_in_memory().unwrap();
    let shapes = format!(
        "@prefix ex: <{EX}> . @prefix sh: <http://www.w3.org/ns/shacl#> .
         @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
         ex:S a sh:NodeShape; sh:targetClass ex:C;
         sh:property ex:Local, ex:Nonlocal.
         ex:Local sh:path ex:r; sh:not [sh:datatype xsd:string].
         ex:Nonlocal sh:path ex:p; sh:not [sh:class ex:Wanted]."
    );
    let rdf_type = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    for class in ["Wanted", "C"] {
        let class = store.intern(&format!("{EX}{class}")).unwrap();
        let typed = datum(&store, rdf_type, Value::Ref(class));
        write(&mut store, &shapes, &[typed]).unwrap();
    }
    let item = store.intern(&format!("{EX}item")).unwrap();
    let reference = datum(&store, &format!("{EX}p"), Value::Ref(item));
    // The class-dependent Not remains outside new admission even though the
    // same component is enabled on Local. This pins the source-identity gate.
    write(&mut store, &shapes, &[reference]).unwrap();
    let valid = datum(&store, &format!("{EX}r"), Value::Int(1));
    write(&mut store, &shapes, &[valid]).unwrap();
    let invalid = datum(&store, &format!("{EX}r"), Value::Str("bad".into()));
    assert!(matches!(
        write(&mut store, &shapes, &[invalid]),
        Err(Error::ValidationFailed { .. })
    ));
}

#[test]
fn equality_can_still_be_completed_incrementally_beside_a_local_expression() {
    let mut store = Store::open_in_memory().unwrap();
    let shapes = format!(
        "@prefix ex: <{EX}> . @prefix sh: <http://www.w3.org/ns/shacl#> .
         @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
         ex:S a sh:NodeShape; sh:targetClass ex:C;
         sh:property [sh:path ex:p; sh:equals ex:q],
                     [sh:path ex:r; sh:not [sh:datatype xsd:string]]."
    );
    let class = store.intern(&format!("{EX}C")).unwrap();
    let typed = datum(
        &store,
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
        Value::Ref(class),
    );
    write(&mut store, &shapes, &[typed]).unwrap();
    for (predicate, value) in [("p", "x"), ("q", "y"), ("p", "y"), ("q", "x")] {
        let value = datum(
            &store,
            &format!("{EX}{predicate}"),
            Value::Str(value.into()),
        );
        write(&mut store, &shapes, &[value]).unwrap();
    }
    let valid = datum(&store, &format!("{EX}r"), Value::Int(1));
    write(&mut store, &shapes, &[valid]).unwrap();
}

fn datum(store: &Store, predicate: &str, value: Value) -> Datum {
    Datum {
        entity: store.intern(&format!("{EX}item")).unwrap(),
        attribute: store.intern(predicate).unwrap(),
        value,
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

fn write(store: &mut Store, shapes: &str, datums: &[Datum]) -> quipu::Result<i64> {
    store.with_write_shapes(Some(shapes), |store| {
        store.transact_to_graph(datums, TS, None, None, 0)
    })
}

#[test]
fn value_local_failures_reject_before_or_after_targeting_and_valid_values_pass() {
    let cases = [
        (
            "sh:or ([sh:datatype xsd:integer] [sh:datatype xsd:boolean])",
            Value::Str("bad".into()),
        ),
        (
            "sh:and ([sh:datatype xsd:integer] [sh:minInclusive 0])",
            Value::Int(-1),
        ),
        ("sh:not [sh:datatype xsd:string]", Value::Str("bad".into())),
        (
            "sh:xone ([sh:datatype xsd:integer] [sh:datatype xsd:boolean])",
            Value::Str("bad".into()),
        ),
    ];
    for (constraint, bad) in cases {
        for valid in [false, true] {
            for type_first in [false, true] {
                let mut store = Store::open_in_memory().unwrap();
                let shapes = format!(
                    "@prefix ex: <{EX}> . @prefix sh: <http://www.w3.org/ns/shacl#> .
                     @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
                     ex:S a sh:NodeShape; sh:targetClass ex:C;
                     sh:property [sh:path ex:p; {constraint}]."
                );
                let class = store.intern(&format!("{EX}C")).unwrap();
                let typed = datum(
                    &store,
                    "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
                    Value::Ref(class),
                );
                let value = datum(
                    &store,
                    &format!("{EX}p"),
                    if valid { Value::Int(1) } else { bad.clone() },
                );
                let (first, last) = if type_first {
                    (typed, value)
                } else {
                    (value, typed)
                };
                write(&mut store, &shapes, &[first]).unwrap();
                let result = write(&mut store, &shapes, &[last]);
                if valid {
                    result.unwrap();
                } else {
                    assert!(
                        matches!(result, Err(Error::ValidationFailed { .. })),
                        "{constraint}: {result:?}"
                    );
                    let subject = store.lookup(&format!("{EX}item")).unwrap().unwrap();
                    assert_eq!(
                        store.entity_facts(subject).unwrap().len(),
                        1,
                        "rejected write must roll back"
                    );
                }
            }
        }
    }
}

#[test]
fn closed_and_qualified_max_reject_late_invalid_sets_without_requiring_values() {
    for qualified in [false, true] {
        for valid in [false, true] {
            for type_first in [false, true] {
                let mut store = Store::open_in_memory().unwrap();
                let component = if qualified {
                    "sh:property [sh:path ex:p; sh:qualifiedValueShape [sh:datatype xsd:string]; sh:qualifiedMaxCount 1]"
                } else {
                    "sh:closed true; sh:ignoredProperties (rdf:type); sh:property [sh:path ex:p]"
                };
                let shapes = format!(
                    "@prefix ex: <{EX}> . @prefix sh: <http://www.w3.org/ns/shacl#> .
                     @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
                     @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
                     ex:S a sh:NodeShape; sh:targetClass ex:C; {component}."
                );
                let class = store.intern(&format!("{EX}C")).unwrap();
                let typed = datum(
                    &store,
                    "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
                    Value::Ref(class),
                );
                let first = datum(&store, &format!("{EX}p"), Value::Str("one".into()));
                let second = datum(
                    &store,
                    &format!("{EX}{}", if !qualified && !valid { "q" } else { "p" }),
                    if qualified && valid {
                        Value::Int(2)
                    } else {
                        Value::Str("two".into())
                    },
                );
                let result = if type_first {
                    write(&mut store, &shapes, &[typed]).unwrap();
                    write(&mut store, &shapes, &[first]).unwrap();
                    write(&mut store, &shapes, &[second])
                } else {
                    write(&mut store, &shapes, &[first]).unwrap();
                    write(&mut store, &shapes, &[second]).unwrap();
                    write(&mut store, &shapes, &[typed])
                };
                if valid {
                    result.unwrap();
                } else {
                    assert!(
                        matches!(result, Err(Error::ValidationFailed { .. })),
                        "qualified={qualified}: {result:?}"
                    );
                }
            }
        }
    }
}
