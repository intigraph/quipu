//! Wire v2 (decision-deck-seed-decisions.md §1 and §4): labelled choices,
//! Later and comment. Temporary stores and software authenticators only.
use super::tests::{config, refused, signed};
use super::*;
use crate::verdict_schemes::webauthn_tests::{Authenticator, Ceremony};

const ID: &str = "urn:test:item-question";

fn v2(revision: u32, actions: &[&str]) -> Request {
    Request {
        id: ID.into(),
        revision,
        policy: "test-only-policy".into(),
        target: "urn:test:item".into(),
        summary: "Which window should the upgrade use?".into(),
        expires_at: now().unwrap() + 3600,
        choices: Vec::new(),
        author: "test-curator".into(),
        labels: vec!["Tonight".into(), "This weekend".into()],
        actions: actions.iter().map(|a| (*a).to_string()).collect(),
    }
}

const ALL: [&str; 3] = ["choose", "later", "comment"];

fn prepare(store: &mut Store, auth: &Authenticator) {
    store.deck_configure(&config()).unwrap();
    store
        .deck_enroll(&Credential {
            id: "dGVzdA".into(),
            scheme: auth.scheme().tag().into(),
            cose_key: auth.cose_key(),
            expires_at: now().unwrap() + 3600,
        })
        .unwrap();
    store.deck_put_request(&v2(1, &ALL)).unwrap();
    store.deck_set_enabled(true).unwrap();
}

/// Receipts, challenges and deck-graph facts: a refusal changes none of them.
fn footprint(store: &Store) -> (i64, i64, usize) {
    let count = |table: &str| -> i64 {
        store
            .conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };
    let graph = store.intern(GRAPH).unwrap();
    (
        count("deck_receipts"),
        count("deck_challenges"),
        store.current_facts_in_graph(graph).unwrap().len(),
    )
}

/// The string value of one Decision field, if recorded.
fn field(store: &Store, decision: &str, name: &str) -> Option<String> {
    let graph = store.intern(GRAPH).unwrap();
    let entity = store.intern(decision).unwrap();
    let attribute = store.intern(&format!("{DEFAULT_BASE_NS}{name}")).unwrap();
    store
        .current_facts_in_graph(graph)
        .unwrap()
        .into_iter()
        .find(|f| f.entity == entity && f.attribute == attribute)
        .map(|f| match f.value {
            Value::Str(s) => s,
            other => panic!("{name}: {other:?}"),
        })
}

#[test]
fn a_request_is_exactly_one_wire() {
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &Authenticator::es256());
    let bad = [
        Request {
            choices: vec!["approve".into()],
            ..v2(2, &ALL)
        },
        Request {
            labels: Vec::new(),
            ..v2(2, &ALL)
        },
        Request {
            actions: Vec::new(),
            ..v2(2, &ALL)
        },
        Request {
            actions: vec!["execute".into()],
            ..v2(2, &ALL)
        },
        Request {
            actions: vec!["later".into(), "later".into()],
            ..v2(2, &ALL)
        },
        Request {
            labels: vec!["A".into(), "A".into()],
            ..v2(2, &ALL)
        },
        Request {
            labels: vec!["line\nbreak".into()],
            ..v2(2, &ALL)
        },
        Request {
            labels: vec![String::new()],
            ..v2(2, &ALL)
        },
        Request {
            labels: vec!["x".repeat(201)],
            ..v2(2, &ALL)
        },
        Request {
            labels: (0..9).map(|i| i.to_string()).collect(),
            ..v2(2, &ALL)
        },
    ];
    for request in bad {
        refused(s.deck_put_request(&request), "invalid request");
    }
    // Controls: the limits themselves are admitted.
    s.deck_put_request(&Request {
        labels: (0..8).map(|i| i.to_string()).collect(),
        ..v2(2, &ALL)
    })
    .unwrap();
    s.deck_put_request(&Request {
        labels: vec!["x".repeat(200)],
        ..v2(3, &["later"])
    })
    .unwrap();
}

#[test]
fn choose_later_and_comment_each_record_their_signed_fields() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &a);
    let cases = [
        ("choose", "This weekend", Some("after the backup")),
        ("later", "", None),
        ("comment", "", Some("Ask me after the backup finishes.")),
    ];
    for (revision, (action, choice, comment)) in (1u32..).zip(cases) {
        if revision > 1 {
            s.deck_put_request(&v2(revision, &ALL)).unwrap();
        }
        let c = s
            .deck_present_action(ID, revision, "dGVzdA", Some(action), choice, comment)
            .unwrap();
        assert_eq!(c.domain, "quipu-decision-deck-v2");
        let message = String::from_utf8(c.message().unwrap()).unwrap();
        if let Some(comment) = comment {
            assert!(
                message.contains(comment),
                "the comment is signed: {message}"
            );
        }
        // One credential signs every revision, so its counter must advance.
        let mut ceremony = Ceremony::for_message(&c.message().unwrap());
        ceremony.sign_count = 7 + revision;
        let receipt = s
            .deck_attest(&c.nonce, &a.sign(&ceremony).as_assertion())
            .unwrap();
        assert!(s.deck_verify_receipt(&c.nonce).unwrap());
        let d = &receipt.decision;
        assert_eq!(field(&s, d, "outcome").as_deref(), Some(action));
        assert_eq!(field(&s, d, "deckAction").as_deref(), Some(action));
        let label = (action == "choose").then_some(choice);
        assert_eq!(field(&s, d, "deckLabel").as_deref(), label);
        assert_eq!(field(&s, d, "deckComment").as_deref(), comment);
    }
}

#[test]
fn an_undeclared_selection_is_refused_and_records_nothing() {
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &Authenticator::es256());
    let long = "x".repeat(4001);
    let bad: [(Option<&str>, &str, Option<&str>); 11] = [
        (Some("choose"), "Next month", None),
        (Some("choose"), "tonight", None),
        (Some("choose"), "", None),
        (Some("later"), "Tonight", None),
        (Some("comment"), "", None),
        (Some("comment"), "", Some("  \n ")),
        (Some("comment"), "Tonight", Some("why not")),
        (Some("comment"), "", Some(long.as_str())),
        (Some("later"), "", Some("bell\u{7}")),
        (Some("execute"), "", None),
        (None, "Tonight", None),
    ];
    let before = footprint(&s);
    for (action, choice, comment) in bad {
        refused(
            s.deck_present_action(ID, 1, "dGVzdA", action, choice, comment),
            "undeclared",
        );
    }
    // A v1 present names no action, so a v2 request refuses it.
    refused(s.deck_present(ID, 1, "dGVzdA", "Tonight"), "undeclared");
    assert_eq!(footprint(&s), before);
    // Controls: the comment limit itself is admitted, as are tabs and newlines.
    let limit = "x".repeat(4000);
    s.deck_present_action(ID, 1, "dGVzdA", Some("comment"), "", Some(&limit))
        .unwrap();
    s.deck_present_action(ID, 1, "dGVzdA", Some("later"), "", Some("a\tb\nc"))
        .unwrap();
}

#[test]
fn an_action_the_request_does_not_allow_is_refused() {
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &Authenticator::es256());
    s.deck_put_request(&v2(2, &["choose"])).unwrap();
    let before = footprint(&s);
    refused(
        s.deck_present_action(ID, 2, "dGVzdA", Some("later"), "", None),
        "undeclared",
    );
    refused(
        s.deck_present_action(ID, 2, "dGVzdA", Some("comment"), "", Some("hm")),
        "undeclared",
    );
    assert_eq!(footprint(&s), before);
    s.deck_present_action(ID, 2, "dGVzdA", Some("choose"), "Tonight", None)
        .unwrap();
}

#[test]
fn a_v1_request_takes_no_action_or_comment() {
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &Authenticator::es256());
    let v1 = Request {
        id: "urn:test:v1".into(),
        choices: vec!["approve".into(), "reject".into()],
        labels: Vec::new(),
        actions: Vec::new(),
        ..v2(1, &ALL)
    };
    s.deck_put_request(&v1).unwrap();
    let before = footprint(&s);
    refused(
        s.deck_present_action(&v1.id, 1, "dGVzdA", Some("choose"), "approve", None),
        "undeclared",
    );
    refused(
        s.deck_present_action(&v1.id, 1, "dGVzdA", None, "approve", Some("note")),
        "undeclared",
    );
    assert_eq!(footprint(&s), before);
    let c = s.deck_present(&v1.id, 1, "dGVzdA", "approve").unwrap();
    assert_eq!(c.domain, "quipu-decision-deck-v1");
    assert!(c.action.is_none() && c.comment.is_none());
}

#[test]
fn a_signature_over_another_comment_is_refused_and_records_nothing() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &a);
    let c = s
        .deck_present_action(ID, 1, "dGVzdA", Some("comment"), "", Some("the real text"))
        .unwrap();
    let forged = Challenge {
        comment: Some("a different text".into()),
        ..c.clone()
    };
    let before = footprint(&s);
    assert!(
        s.deck_attest(&c.nonce, &signed(&a, &forged).as_assertion())
            .is_err()
    );
    assert_eq!(footprint(&s), before);
    // Control: the genuine signature is admitted.
    s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion())
        .unwrap();
}
