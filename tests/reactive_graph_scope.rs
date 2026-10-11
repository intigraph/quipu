#![cfg(feature = "reactive-reasoner")]
//! Reactive rules follow the committed graph, including retractions and batches.
use quipu::{
    reasoner::{parse_rules, reactive::ReactiveReasoner},
    store::{Datum, Store},
    types::{Op, Value},
};
use std::sync::Arc;
const TS: &str = "2026-10-11T00:00:00Z";
fn store() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    let rules = parse_rules(
        r#"@prefix rule: <http://quipu.local/rule#> .
 <https://example.org/rule> a rule:Rule ; rule:id "graph-scope-test" ;
 rule:head "<https://example.org/head>(?s, ?o)" ;
 rule:body "<https://example.org/premise>(?s, ?o)" ."#,
        None,
    )
    .unwrap();
    s.add_observer(Arc::new(ReactiveReasoner::new(rules)));
    s
}
fn datum(s: &mut Store, subject: &str, object: &str, op: Op) -> Datum {
    Datum {
        entity: s.intern(subject).unwrap(),
        attribute: s.intern("https://example.org/premise").unwrap(),
        value: Value::Ref(s.intern(object).unwrap()),
        valid_from: TS.into(),
        valid_to: None,
        op,
    }
}
fn change(s: &mut Store, g: i64, subject: &str, object: &str, op: Op) {
    let d = datum(s, subject, object, op);
    s.transact_to_graph(&[d], TS, Some("test"), Some("test"), g)
        .unwrap();
}
fn heads(s: &Store, g: i64) -> Vec<(i64, Value)> {
    let iri = s.companion_inferred_iri(g).unwrap();
    let Some(companion) = s.lookup(&iri).unwrap() else {
        return vec![];
    };
    let Some(head) = s.lookup("https://example.org/head").unwrap() else {
        return vec![];
    };
    s.current_facts_in_graph(companion)
        .unwrap()
        .into_iter()
        .filter(|f| f.attribute == head)
        .map(|f| (f.entity, f.value))
        .collect()
}
#[test]
fn named_graphs_and_root_derive_independently() {
    let mut s = store();
    let a = s.intern("https://example.org/project/a").unwrap();
    let b = s.intern("https://example.org/project/b").unwrap();
    change(
        &mut s,
        a,
        "https://example.org/shared",
        "https://example.org/only-a",
        Op::Assert,
    );
    assert_eq!(heads(&s, a).len(), 1);
    assert!(heads(&s, b).is_empty());
    assert!(heads(&s, 0).is_empty());
    change(
        &mut s,
        b,
        "https://example.org/shared",
        "https://example.org/only-b",
        Op::Assert,
    );
    assert_eq!(heads(&s, a).len(), 1);
    assert_eq!(heads(&s, b).len(), 1);
    assert_ne!(heads(&s, a), heads(&s, b));
    change(
        &mut s,
        0,
        "https://example.org/root",
        "https://example.org/root-object",
        Op::Assert,
    );
    assert_eq!(heads(&s, 0).len(), 1);
    assert_eq!(heads(&s, a).len(), 1);
    assert_eq!(heads(&s, b).len(), 1);
}
#[test]
fn named_retraction_does_not_retract_another_projects_derivation() {
    let mut s = store();
    let a = s.intern("https://example.org/project/a").unwrap();
    let b = s.intern("https://example.org/project/b").unwrap();
    for g in [a, b] {
        change(
            &mut s,
            g,
            "https://example.org/shared",
            "https://example.org/object",
            Op::Assert,
        );
    }
    assert_eq!(heads(&s, a).len(), 1);
    assert_eq!(heads(&s, b).len(), 1);
    change(
        &mut s,
        a,
        "https://example.org/shared",
        "https://example.org/object",
        Op::Retract,
    );
    assert!(heads(&s, a).is_empty());
    assert_eq!(heads(&s, b).len(), 1);
}
#[test]
fn multi_graph_batch_notifies_each_committed_graph() {
    let mut s = store();
    let a = s.intern("https://example.org/project/a").unwrap();
    let b = s.intern("https://example.org/project/b").unwrap();
    let da = datum(
        &mut s,
        "https://example.org/a",
        "https://example.org/object",
        Op::Assert,
    );
    let db = datum(
        &mut s,
        "https://example.org/b",
        "https://example.org/object",
        Op::Assert,
    );
    s.transact_graph_batches(
        &[(a, vec![da]), (b, vec![db])],
        TS,
        Some("test"),
        Some("test"),
    )
    .unwrap();
    assert_eq!(heads(&s, a).len(), 1);
    assert_eq!(heads(&s, b).len(), 1);
    assert!(heads(&s, 0).is_empty());
}
