//! `member` conditions (aegis-n0wc.3): N-Triples parsing, exact-term
//! matching, before-OR-after evaluation, graph scope and the store walk.

use super::*;
use crate::store::{Datum, Store};
use crate::types::Op;

const ASSIGNEE: &str = "urn:p:assignee";
const STATUS: &str = "urn:p:status";

fn datum(store: &Store, e: &str, a: &str, value: Value, op: Op) -> Datum {
    Datum {
        entity: store.intern(e).unwrap(),
        attribute: store.intern(a).unwrap(),
        value,
        valid_from: "2026-01-01".into(),
        valid_to: None,
        op,
    }
}

fn str_(s: &str) -> Value {
    Value::Str(s.into())
}

fn condition(store: &Store, text: &str) -> MemberCondition {
    store
        .member_condition(&MemberSpec::parse(text).unwrap())
        .unwrap()
        .expect("known predicate and value")
}

fn admitted(store: &Store, tx: i64, cond: &MemberCondition, g: i64, e: &str) -> bool {
    let e = store.lookup(e).unwrap().unwrap();
    store
        .member_pairs(tx, cond, &[(g, e)])
        .unwrap()
        .0
        .contains(&(g, e))
}

#[test]
fn changes_member_spec_parses_n_triples_and_normalizes() {
    let spec = MemberSpec::parse("  <urn:p:assignee>   \"alice\" ").unwrap();
    assert_eq!(spec.predicate, ASSIGNEE);
    assert_eq!(spec.term, "\"alice\"");
    let xsd_string = format!("<{ASSIGNEE}> \"alice\"^^<{}>", crate::namespace::XSD_STRING);
    assert_eq!(
        MemberSpec::parse(&xsd_string).unwrap(),
        spec,
        "xsd:string is the plain literal"
    );
    assert_eq!(
        MemberSpec::parse("<urn:p> \"x\"@EN").unwrap().term,
        "\"x\"@en"
    );
    assert_eq!(
        MemberSpec::parse("<urn:p> \"5\"^^<urn:t>").unwrap().term,
        "\"5\"^^<urn:t>"
    );
    assert_eq!(
        MemberSpec::parse("<urn:p> <urn:Class>").unwrap().term,
        "<urn:Class>"
    );
    for bad in [
        "",
        "urn:p \"a\"",
        "<urn:p>",
        "<urn:p> alice",
        "<urn:p> 5",
        "<urn:p> true",
        "<urn:p> _:b0",
        "<urn:p> xsd:int",
        "<urn:p> \"a\" \"b\"",
        "<urn:p> \"unterminated",
        "<urn:p <urn:v>",
    ] {
        assert!(MemberSpec::parse(bad).is_err(), "{bad:?} must be refused");
    }
    let long = format!("<urn:p> \"{}\"", "x".repeat(MAX_MEMBER_LEN));
    assert!(MemberSpec::parse(&long).is_err());
}

/// Assign A, change status, reassign to B, change status again: the
/// condition `assignee "A"` holds after tx1, before and after tx2, before tx3
/// (only), and at neither side of tx4.
#[test]
fn changes_member_holds_before_or_after_a_transaction() {
    let mut store = Store::open_in_memory().unwrap();
    let item = "urn:item:1";
    let tx1 = store
        .transact(
            &[datum(&store, item, ASSIGNEE, str_("A"), Op::Assert)],
            "2026-01-01",
            None,
            None,
        )
        .unwrap();
    let tx2 = store
        .transact(
            &[datum(&store, item, STATUS, str_("open"), Op::Assert)],
            "2026-01-02",
            None,
            None,
        )
        .unwrap();
    let tx3 = store
        .transact(
            &[
                datum(&store, item, ASSIGNEE, str_("A"), Op::Retract),
                datum(&store, item, ASSIGNEE, str_("B"), Op::Assert),
            ],
            "2026-01-03",
            None,
            None,
        )
        .unwrap();
    let tx4 = store
        .transact(
            &[datum(&store, item, STATUS, str_("done"), Op::Assert)],
            "2026-01-04",
            None,
            None,
        )
        .unwrap();
    let a = condition(&store, "<urn:p:assignee> \"A\"");
    let b = condition(&store, "<urn:p:assignee> \"B\"");
    assert!(admitted(&store, tx1, &a, 0, item), "after tx1");
    assert!(admitted(&store, tx2, &a, 0, item), "before and after tx2");
    assert!(
        admitted(&store, tx3, &a, 0, item),
        "before tx3: the move-out signal"
    );
    assert!(!admitted(&store, tx4, &a, 0, item), "neither side of tx4");
    assert!(!admitted(&store, tx2, &b, 0, item));
    assert!(admitted(&store, tx3, &b, 0, item), "after tx3");
    assert!(admitted(&store, tx4, &b, 0, item));
}

/// Exact terms: `"A"` matches neither `"A"^^<urn:t>` nor `"A"@en`, and
/// each of those matches only itself.
#[test]
fn changes_member_matches_an_exact_rdf_term() {
    let mut store = Store::open_in_memory().unwrap();
    let typed = Value::Typed {
        lexical: "A".into(),
        datatype: "urn:t".into(),
    };
    let lang = Value::Lang {
        lexical: "A".into(),
        lang: "en".into(),
    };
    let tx = store
        .transact(
            &[
                datum(&store, "urn:item:typed", ASSIGNEE, typed, Op::Assert),
                datum(&store, "urn:item:lang", ASSIGNEE, lang, Op::Assert),
                datum(&store, "urn:item:plain", ASSIGNEE, str_("A"), Op::Assert),
                datum(
                    &store,
                    "urn:item:ref",
                    ASSIGNEE,
                    Value::Ref(store.intern("urn:A").unwrap()),
                    Op::Assert,
                ),
            ],
            "2026-01-01",
            None,
            None,
        )
        .unwrap();
    let cases = [
        ("\"A\"", "urn:item:plain"),
        ("\"A\"^^<urn:t>", "urn:item:typed"),
        ("\"A\"@en", "urn:item:lang"),
        ("<urn:A>", "urn:item:ref"),
    ];
    for (term, owner) in cases {
        let cond = condition(&store, &format!("<{ASSIGNEE}> {term}"));
        for (_, item) in cases {
            assert_eq!(
                admitted(&store, tx, &cond, 0, item),
                item == owner,
                "{term} on {item}"
            );
        }
    }
    // An IRI value or predicate the store has never seen holds for nothing.
    for unknown in ["<urn:p:assignee> <urn:never>", "<urn:p:never> \"A\""] {
        let spec = MemberSpec::parse(unknown).unwrap();
        assert!(
            store.member_condition(&spec).unwrap().is_none(),
            "{unknown}"
        );
    }
}

/// A membership fact in graph Y admits the subject in Y only.
#[test]
fn changes_member_is_evaluated_in_one_graph() {
    let mut store = Store::open_in_memory().unwrap();
    let x = store.graph_create("urn:graph:x").unwrap();
    let y = store.graph_create("urn:graph:y").unwrap();
    let item = "urn:item:1";
    store
        .transact_to_graph(
            &[datum(&store, item, ASSIGNEE, str_("A"), Op::Assert)],
            "2026-01-01",
            None,
            None,
            y,
        )
        .unwrap();
    let tx = store
        .transact_to_graph(
            &[datum(&store, item, STATUS, str_("open"), Op::Assert)],
            "2026-01-02",
            None,
            None,
            x,
        )
        .unwrap();
    let a = condition(&store, "<urn:p:assignee> \"A\"");
    assert!(admitted(&store, tx, &a, y, item));
    assert!(
        !admitted(&store, tx, &a, x, item),
        "a fact in Y never admits in X"
    );
    assert!(!admitted(&store, tx, &a, 0, item), "nor in ROOT");
}

/// The store walk keeps only admitted records, skips transactions with none
/// and stops at the page limit.
#[test]
fn changes_member_store_page_skips_transactions_without_members() {
    let mut store = Store::open_in_memory().unwrap();
    let mut write = |data: &[(&str, &str, &str, Op)]| {
        let data: Vec<Datum> = data
            .iter()
            .map(|(e, a, v, op)| datum(&store, e, a, str_(v), *op))
            .collect();
        store.transact(&data, "2026-01-05", None, None).unwrap()
    };
    let start = write(&[("urn:seed", STATUS, "x", Op::Assert)]);
    let assign_a = write(&[
        ("urn:a", ASSIGNEE, "A", Op::Assert),
        ("urn:b", ASSIGNEE, "B", Op::Assert),
    ]);
    let b_status = write(&[("urn:b", STATUS, "open", Op::Assert)]);
    let a_status = write(&[
        ("urn:a", STATUS, "open", Op::Assert),
        ("urn:b", STATUS, "x", Op::Retract),
    ]);
    let cond = condition(&store, "<urn:p:assignee> \"A\"");
    let page = store
        .changes_after_member(
            start,
            1,
            Capture::OldAndNewValues,
            None,
            &TermScope::default(),
            &cond,
        )
        .unwrap();
    assert_eq!(page.next_tx, assign_a);
    assert_eq!(
        page.records.len(),
        1,
        "only urn:a's record: {:?}",
        page.records
    );
    assert_eq!(page.records[0]["entity"], "urn:a");
    let page = store
        .changes_after_member(
            assign_a,
            1,
            Capture::OldAndNewValues,
            None,
            &TermScope::default(),
            &cond,
        )
        .unwrap();
    assert_eq!(
        page.next_tx, a_status,
        "b's status transaction {b_status} is skipped"
    );
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0]["sequence"], 0, "renumbered from 0");
    let page = store
        .changes_after_member(
            a_status,
            1,
            Capture::OldAndNewValues,
            None,
            &TermScope::default(),
            &cond,
        )
        .unwrap();
    assert!(page.records.is_empty());
    assert_eq!(page.next_tx, a_status, "an exhausted walk stays put");
}

/// A walk that reaches the end of the log past transactions with no member
/// moves its cursor to the last one walked, so the next read does not walk
/// them again; with nothing to walk it stays put.
#[test]
fn changes_member_walk_to_the_end_moves_past_unmatched_transactions() {
    let mut store = Store::open_in_memory().unwrap();
    let mut write = |data: &[(&str, &str, &str, Op)]| {
        let data: Vec<Datum> = data
            .iter()
            .map(|(e, a, v, op)| datum(&store, e, a, str_(v), *op))
            .collect();
        store.transact(&data, "2026-01-05", None, None).unwrap()
    };
    let start = write(&[("urn:b", ASSIGNEE, "B", Op::Assert)]);
    write(&[("urn:b", STATUS, "open", Op::Assert)]);
    let last = write(&[("urn:c", STATUS, "open", Op::Assert)]);
    let cond = condition(&store, "<urn:p:assignee> \"A\"");
    let page = |since| {
        store
            .changes_after_member(
                since,
                1,
                Capture::OldAndNewValues,
                None,
                &TermScope::default(),
                &cond,
            )
            .unwrap()
    };
    let walked = page(start);
    assert!(walked.records.is_empty());
    assert_eq!(
        walked.next_tx, last,
        "moved past the unmatched transactions"
    );
    assert_eq!(page(last).next_tx, last, "nothing to walk: stays put");
}
