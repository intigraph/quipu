#![cfg(all(feature = "reactive-reasoner", feature = "shacl"))]
//! Request shapes judge the caller's datums, not derivations made after the
//! commit (aegis-2i6). Inheriting them refused a derived write about another
//! node and answered 400 for a write that had already landed.
use quipu::{
    reasoner::{parse_rules, reactive::ReactiveReasoner},
    store::{Datum, Store},
    types::{Op, Value},
};
use std::sync::Arc;

const TS: &str = "2026-10-21T00:00:00Z";
const BOUNDED_HEAD: &str = r#"
@prefix ex: <https://example.org/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
ex:HeadShape a sh:NodeShape; sh:targetSubjectsOf ex:head;
 sh:property [sh:path ex:head; sh:maxCount 1].
"#;

fn store() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    let rules = parse_rules(
        r#"@prefix rule: <http://quipu.local/rule#> .
 <https://example.org/rule> a rule:Rule ; rule:id "request-shapes-test" ;
 rule:head "<https://example.org/head>(?s, ?o)" ;
 rule:body "<https://example.org/premise>(?s, ?o)" ."#,
        None,
    )
    .unwrap();
    s.add_observer(Arc::new(ReactiveReasoner::new(rules)));
    s
}

fn premise(s: &Store, object: &str) -> Datum {
    Datum {
        entity: s.intern("https://example.org/x").unwrap(),
        attribute: s.intern("https://example.org/premise").unwrap(),
        value: Value::Ref(s.intern(&format!("https://example.org/{object}")).unwrap()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

#[test]
fn a_derivation_is_not_judged_by_the_request_shapes() {
    let mut s = store();
    let first = premise(&s, "a");
    s.transact(&[first], TS, Some("test"), Some("test"))
        .unwrap();
    // The caller's datum is unbounded; the rule then derives a second head
    // for x, which the request's shape bounds at one.
    let second = premise(&s, "b");
    s.with_write_shapes(Some(BOUNDED_HEAD), |s| {
        s.transact(&[second], TS, Some("test"), Some("test"))
    })
    .unwrap();
    let x = s.lookup("https://example.org/x").unwrap().unwrap();
    let premise_id = s.lookup("https://example.org/premise").unwrap().unwrap();
    let premises = s
        .current_facts_in_graph(0)
        .unwrap()
        .into_iter()
        .filter(|f| f.entity == x && f.attribute == premise_id)
        .count();
    assert_eq!(premises, 2, "the caller's write committed");
}
