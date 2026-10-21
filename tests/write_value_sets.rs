//! Values written separately must receive the same set constraints as a batch.
#![cfg(feature = "shacl")]
use quipu::{Datum, Error, Op, Store, Value};

const TS: &str = "2026-10-21T00:00:00Z";
const NS: &str = "https://example.org/value-sets/";

fn shapes(constraint: &str) -> String {
    format!(
        "@prefix ex: <{NS}> . @prefix sh: <http://www.w3.org/ns/shacl#> .
         ex:S a sh:NodeShape; sh:targetNode ex:focus;
         sh:property [sh:path ex:left; {constraint}]."
    )
}
fn datum(s: &Store, predicate: &str, value: Value, op: Op) -> Datum {
    Datum {
        entity: s.intern(&format!("{NS}focus")).unwrap(),
        attribute: s.intern(&format!("{NS}{predicate}")).unwrap(),
        value,
        valid_from: TS.into(),
        valid_to: None,
        op,
    }
}
fn write(s: &mut Store, shapes: &str, data: &[Datum], graph: i64) -> quipu::Result<i64> {
    s.with_write_shapes(Some(shapes), |s| {
        s.transact_to_graph(data, TS, None, None, graph)
    })
}
fn terms(s: &Store, graph: i64) -> std::collections::BTreeSet<(i64, Vec<u8>)> {
    let focus = s.lookup(&format!("{NS}focus")).unwrap().unwrap();
    s.entity_facts_in_graph(focus, graph)
        .unwrap()
        .into_iter()
        .map(|f| (f.attribute, f.value.to_bytes()))
        .collect()
}
fn lang(word: &str, language: &str) -> Value {
    Value::Lang {
        lexical: word.into(),
        lang: language.into(),
    }
}

#[test]
fn unique_language_split_rejects_and_rolls_back_in_both_orders() {
    for reverse in [false, true] {
        let mut s = Store::open_in_memory().unwrap();
        let shape = shapes("sh:uniqueLang true");
        let mut values = [lang("first", "en"), lang("second", "en")];
        if reverse {
            values.reverse();
        }
        let first = datum(&s, "left", values[0].clone(), Op::Assert);
        write(&mut s, &shape, std::slice::from_ref(&first), 0).unwrap();
        write(&mut s, &shape, &[first], 0).unwrap(); // identical term is idempotent
        let before = terms(&s, 0);
        let second = datum(&s, "left", values[1].clone(), Op::Assert);
        assert!(matches!(
            write(&mut s, &shape, &[second], 0),
            Err(Error::ValidationFailed { .. })
        ));
        assert_eq!(terms(&s, 0), before);
        let different_language = datum(&s, "left", lang("second", "fr"), Op::Assert);
        write(&mut s, &shape, &[different_language], 0).unwrap();
    }
}

#[test]
fn comparisons_split_reject_in_both_orders_and_named_graphs_stay_isolated() {
    for (constraint, left, right) in [
        ("sh:disjoint ex:right", Value::Int(5), Value::Int(5)),
        ("sh:lessThan ex:right", Value::Int(5), Value::Int(5)),
        ("sh:lessThanOrEquals ex:right", Value::Int(6), Value::Int(5)),
    ] {
        for reverse in [false, true] {
            let mut s = Store::open_in_memory().unwrap();
            let shape = shapes(constraint);
            let g = s.graph_create(&format!("{NS}graph")).unwrap();
            let mut pair = [
                datum(&s, "left", left.clone(), Op::Assert),
                datum(&s, "right", right.clone(), Op::Assert),
            ];
            if reverse {
                pair.reverse();
            }
            write(&mut s, &shape, &pair[..1], g).unwrap();
            let before = terms(&s, g);
            assert!(
                matches!(
                    write(&mut s, &shape, &pair[1..], g),
                    Err(Error::ValidationFailed { .. })
                ),
                "{constraint}, reverse={reverse}"
            );
            assert_eq!(terms(&s, g), before);
            // The opposite side in ROOT must not contaminate named values.
            write(&mut s, &shape, &pair[1..], 0).unwrap();
            assert_eq!(terms(&s, g), before);
        }
    }
}

#[test]
fn valid_comparisons_and_atomic_language_replacement_are_admitted() {
    for constraint in [
        "sh:disjoint ex:right",
        "sh:lessThan ex:right",
        "sh:lessThanOrEquals ex:right",
    ] {
        let mut s = Store::open_in_memory().unwrap();
        let shape = shapes(constraint);
        let left = datum(&s, "left", Value::Int(4), Op::Assert);
        let right = datum(&s, "right", Value::Int(5), Op::Assert);
        write(&mut s, &shape, &[right], 0).unwrap();
        write(&mut s, &shape, &[left], 0).unwrap();
    }
    let mut s = Store::open_in_memory().unwrap();
    let shape = shapes("sh:uniqueLang true");
    let old = datum(&s, "left", lang("old", "en"), Op::Assert);
    write(&mut s, &shape, &[old], 0).unwrap();
    let remove = datum(&s, "left", lang("old", "en"), Op::Retract);
    let add = datum(&s, "left", lang("new", "en"), Op::Assert);
    write(&mut s, &shape, &[remove, add], 0).unwrap();
    assert_eq!(terms(&s, 0).len(), 1);
}

#[test]
fn unsupported_set_paths_refuse_without_committing_partial_evidence() {
    for constraint in [
        "sh:uniqueLang true",
        "sh:disjoint ex:right",
        "sh:lessThan ex:right",
        "sh:lessThanOrEquals ex:right",
    ] {
        let mut s = Store::open_in_memory().unwrap();
        let shape = shapes(constraint).replace("sh:path ex:left", "sh:path (ex:link ex:left)");
        let data = datum(&s, "left", Value::Int(4), Op::Assert);
        assert!(matches!(
            write(&mut s, &shape, &[data], 0),
            Err(Error::CannotVerify(_))
        ));
        assert!(terms(&s, 0).is_empty());
    }
}

#[test]
fn non_strict_equality_and_disabled_language_constraint_remain_valid() {
    let mut s = Store::open_in_memory().unwrap();
    let shape = shapes("sh:lessThanOrEquals ex:right");
    let left = datum(&s, "left", Value::Int(5), Op::Assert);
    let right = datum(&s, "right", Value::Int(5), Op::Assert);
    write(&mut s, &shape, &[left], 0).unwrap();
    write(&mut s, &shape, &[right], 0).unwrap();
    let mut s = Store::open_in_memory().unwrap();
    let shape = shapes("sh:uniqueLang false");
    for value in [lang("first", "en"), lang("second", "en")] {
        let data = datum(&s, "left", value, Op::Assert);
        write(&mut s, &shape, &[data], 0).unwrap();
    }
}
