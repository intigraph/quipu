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

// franklin's #713 counterexamples (aegis-7ur8): a value-set constraint reached
// through sh:node reports NodeConstraintComponent, not the leaf component, so the
// post-state filter must keep it, for the same focus and for a parent link.
fn d(s: &Store, subject: &str, predicate: &str, value: Value) -> Datum {
    Datum {
        entity: s.intern(&format!("{NS}{subject}")).unwrap(),
        attribute: s.intern(&format!("{NS}{predicate}")).unwrap(),
        value,
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}
fn r(s: &Store, entity: &str) -> Value {
    Value::Ref(s.intern(&format!("{NS}{entity}")).unwrap())
}
fn facts_of(s: &Store, entity: &str, predicate: &str) -> usize {
    let e = s.lookup(&format!("{NS}{entity}")).unwrap().unwrap();
    let p = s.lookup(&format!("{NS}{predicate}")).unwrap().unwrap();
    s.entity_facts_in_graph(e, 0)
        .unwrap()
        .into_iter()
        .filter(|f| f.attribute == p)
        .count()
}

#[test]
fn wrapped_same_focus_node_value_set_rejects_a_split_and_admits_a_valid_one() {
    let shape = format!(
        "@prefix ex: <{NS}> . @prefix sh: <http://www.w3.org/ns/shacl#> .
         ex:S a sh:NodeShape; sh:targetNode ex:focus;
         sh:node [sh:property [sh:path ex:left; sh:uniqueLang true]]."
    );
    let mut s = Store::open_in_memory().unwrap();
    {
        let v = [d(&s, "focus", "left", lang("first", "en"))];
        write(&mut s, &shape, &v, 0)
    }
    .unwrap();
    let refused = {
        let v = [d(&s, "focus", "left", lang("second", "en"))];
        write(&mut s, &shape, &v, 0)
    };
    assert!(
        matches!(
            refused,
            Err(Error::ValidationFailed { .. } | Error::CannotVerify(_))
        ),
        "wrapped uniqueLang split was admitted: {refused:?}"
    );
    assert_eq!(
        facts_of(&s, "focus", "left"),
        1,
        "the refused value must not be current"
    );
    // Control: different languages satisfy uniqueLang and must still commit.
    let mut ok = Store::open_in_memory().unwrap();
    {
        let v = [d(&ok, "focus", "left", lang("first", "en"))];
        write(&mut ok, &shape, &v, 0)
    }
    .unwrap();
    {
        let v = [d(&ok, "focus", "left", lang("second", "fr"))];
        write(&mut ok, &shape, &v, 0)
    }
    .unwrap();
    assert_eq!(facts_of(&ok, "focus", "left"), 2);
}

#[test]
fn parent_nested_node_value_set_rejects_a_child_only_split_and_admits_a_valid_one() {
    let shape = format!(
        "@prefix ex: <{NS}> . @prefix sh: <http://www.w3.org/ns/shacl#> .
         ex:S a sh:NodeShape; sh:targetNode ex:parent;
         sh:property [sh:path ex:child; sh:node [sh:property [sh:path ex:name; sh:uniqueLang true]]]."
    );
    let mut s = Store::open_in_memory().unwrap();
    let first = [
        d(&s, "parent", "child", r(&s, "c")),
        d(&s, "c", "name", lang("a", "en")),
    ];
    write(&mut s, &shape, &first, 0).unwrap();
    // The second write touches ONLY the child; the violation belongs to the parent focus.
    let refused = {
        let v = [d(&s, "c", "name", lang("b", "en"))];
        write(&mut s, &shape, &v, 0)
    };
    assert!(
        matches!(
            refused,
            Err(Error::ValidationFailed { .. } | Error::CannotVerify(_))
        ),
        "parent-nested uniqueLang split was admitted: {refused:?}"
    );
    assert_eq!(
        facts_of(&s, "c", "name"),
        1,
        "the refused child value must not be current"
    );
    let mut ok = Store::open_in_memory().unwrap();
    let first = [
        d(&ok, "parent", "child", r(&ok, "c")),
        d(&ok, "c", "name", lang("a", "en")),
    ];
    write(&mut ok, &shape, &first, 0).unwrap();
    {
        let v = [d(&ok, "c", "name", lang("b", "fr"))];
        write(&mut ok, &shape, &v, 0)
    }
    .unwrap();
    assert_eq!(facts_of(&ok, "c", "name"), 2);
}
