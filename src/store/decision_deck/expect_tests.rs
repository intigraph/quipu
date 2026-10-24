//! Live preconditions (decision-deck-seed-decisions.md §2): a stale card, a
//! closed item or a missing slot records nothing, at present or at attest.
use super::tests::{refused, signed};
use super::v2_tests::{ALL, ID, footprint, prepare, v2};
use super::*;
use crate::verdict_schemes::webauthn_tests::Authenticator;

const BOARD: &str = "https://example.org/board";
const ITEM: &str = "https://example.org/item/1";
const VERSION: &str = "https://schema.org/version";
const STATUS: &str = "https://example.org/status";
const OWNER: &str = "https://example.org/owner";

/// Replace the item's single value of `p` in the board graph.
fn set(store: &mut Store, p: &str, old: Option<Value>, new: Value) {
    let g = store.graph_create(BOARD).unwrap();
    let e = store.intern(ITEM).unwrap();
    let a = store.intern(p).unwrap();
    let ts = crate::time::now_iso();
    let mut datums = Vec::new();
    for (value, op) in old
        .map(|v| (v, Op::Retract))
        .into_iter()
        .chain([(new, Op::Assert)])
    {
        datums.push(Datum {
            entity: e,
            attribute: a,
            value,
            valid_from: ts.clone(),
            valid_to: None,
            op,
        });
    }
    store
        .transact_to_graph(&datums, &ts, None, None, g)
        .unwrap();
}

fn expect(p: &str, o: Option<ExpectValue>) -> Expect {
    Expect {
        graph: BOARD.into(),
        s: ITEM.into(),
        p: p.into(),
        o,
    }
}

/// An open item at version 3, curated into revision 2 with both expectations.
fn curated(store: &mut Store, auth: &Authenticator) {
    prepare(store, auth);
    set(store, VERSION, None, Value::Int(3));
    set(store, STATUS, None, Value::Str("open".into()));
    store
        .deck_put_request(&Request {
            expect: vec![
                expect(VERSION, Some(ExpectValue::Int(3))),
                expect(STATUS, Some(ExpectValue::Str("open".into()))),
            ],
            ..v2(2, &ALL)
        })
        .unwrap();
}

fn present(store: &mut Store) -> Result<Challenge> {
    store.deck_present_action(ID, 2, "dGVzdA", Some("later"), "", None)
}

#[test]
fn an_unchanged_item_is_admitted() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    curated(&mut s, &a);
    let c = present(&mut s).unwrap();
    s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion())
        .unwrap();
}

#[test]
fn a_version_moved_between_present_and_attest_records_nothing() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    curated(&mut s, &a);
    let c = present(&mut s).unwrap();
    set(&mut s, VERSION, Some(Value::Int(3)), Value::Int(4));
    let before = footprint(&s);
    refused(
        s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion()),
        "stale request",
    );
    assert_eq!(footprint(&s), before);
    // Nothing was consumed: once the item reads as curated again, the same
    // challenge settles.
    set(&mut s, VERSION, Some(Value::Int(4)), Value::Int(3));
    s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion())
        .unwrap();
}

#[test]
fn a_closed_item_is_refused_before_signing() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    curated(&mut s, &a);
    set(
        &mut s,
        STATUS,
        Some(Value::Str("open".into())),
        Value::Str("closed".into()),
    );
    let before = footprint(&s);
    refused(present(&mut s), "stale request");
    assert_eq!(footprint(&s), before, "no challenge was issued");
}

#[test]
fn an_expected_absence_matches_only_while_absent() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    curated(&mut s, &a);
    s.deck_put_request(&Request {
        expect: vec![expect(OWNER, None)],
        ..v2(3, &ALL)
    })
    .unwrap();
    let c = s
        .deck_present_action(ID, 3, "dGVzdA", Some("later"), "", None)
        .unwrap();
    set(&mut s, OWNER, None, Value::Str("someone".into()));
    refused(
        s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion()),
        "stale request",
    );
    refused(
        s.deck_present_action(ID, 3, "dGVzdA", Some("later"), "", None),
        "stale request",
    );
}

#[test]
fn an_iri_value_and_an_unknown_graph() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    curated(&mut s, &a);
    let person = s.intern("https://example.org/people/stiwi").unwrap();
    set(&mut s, OWNER, None, Value::Ref(person));
    s.deck_put_request(&Request {
        expect: vec![expect(
            OWNER,
            Some(ExpectValue::Iri("https://example.org/people/stiwi".into())),
        )],
        ..v2(3, &ALL)
    })
    .unwrap();
    s.deck_present_action(ID, 3, "dGVzdA", Some("later"), "", None)
        .unwrap();
    // A string with the same text is not the IRI.
    s.deck_put_request(&Request {
        expect: vec![expect(
            OWNER,
            Some(ExpectValue::Str("https://example.org/people/stiwi".into())),
        )],
        ..v2(4, &ALL)
    })
    .unwrap();
    refused(
        s.deck_present_action(ID, 4, "dGVzdA", Some("later"), "", None),
        "stale request",
    );
    // An unregistered graph is never read as empty.
    s.deck_put_request(&Request {
        expect: vec![Expect {
            graph: "https://example.org/nowhere".into(),
            ..expect(OWNER, None)
        }],
        ..v2(5, &ALL)
    })
    .unwrap();
    refused(
        s.deck_present_action(ID, 5, "dGVzdA", Some("later"), "", None),
        "stale request",
    );
}

#[test]
fn expectations_are_bounded_named_and_v2_only() {
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &Authenticator::es256());
    let many: Vec<Expect> = (0..17)
        .map(|i| expect(&format!("https://example.org/p{i}"), None))
        .collect();
    let bad = [
        Request {
            expect: many.clone(),
            ..v2(2, &ALL)
        },
        Request {
            expect: vec![expect(STATUS, None), expect(STATUS, None)],
            ..v2(2, &ALL)
        },
        Request {
            expect: vec![expect("not an iri", None)],
            ..v2(2, &ALL)
        },
        Request {
            expect: vec![expect(OWNER, Some(ExpectValue::Iri("not an iri".into())))],
            ..v2(2, &ALL)
        },
        Request {
            expect: vec![expect(STATUS, None)],
            choices: vec!["approve".into()],
            labels: Vec::new(),
            actions: Vec::new(),
            ..v2(2, &ALL)
        },
    ];
    for request in bad {
        refused(s.deck_put_request(&request), "invalid request");
    }
    // Control: sixteen is admitted.
    s.deck_put_request(&Request {
        expect: many[..16].to_vec(),
        ..v2(2, &ALL)
    })
    .unwrap();
}
