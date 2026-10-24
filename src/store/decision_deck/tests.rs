//! Temporary stores and software authenticators only; no human enrollment.
use super::*;
use crate::verdict_schemes::webauthn_tests::{Authenticator, Ceremony, ORIGIN, RP_ID, Signed};

pub(super) fn config() -> Config {
    Config {
        origin: ORIGIN.into(),
        rp_id: RP_ID.into(),
        actor: "test-human".into(),
        author: "test-curator".into(),
    }
}

fn request() -> Request {
    Request {
        id: "urn:test:curated-request".into(),
        revision: 1,
        policy: "test-only-policy".into(),
        target: "urn:test:target".into(),
        summary: "Approve the harmless fixture?".into(),
        expires_at: now().unwrap() + 3600,
        choices: vec!["approve".into(), "reject".into()],
        author: "test-curator".into(),
        labels: Vec::new(),
        actions: Vec::new(),
    }
}

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
    store.deck_put_request(&request()).unwrap();
    store.deck_set_enabled(true).unwrap();
}

fn present(store: &mut Store) -> Challenge {
    store
        .deck_present("urn:test:curated-request", 1, "dGVzdA", "approve")
        .unwrap()
}

pub(super) fn signed(auth: &Authenticator, c: &Challenge) -> Signed {
    auth.sign(&Ceremony::for_message(&c.message().unwrap()))
}

pub(super) fn refused<T: std::fmt::Debug>(r: Result<T>, fragment: &str) {
    let e = r.unwrap_err().to_string();
    assert!(e.contains(fragment), "{e} lacks {fragment}");
}

#[test]
fn default_off_and_operator_scope_are_required() {
    let mut s = Store::open_in_memory().unwrap();
    refused(s.deck_present("x", 1, "x", "approve"), "disabled");
    let mut c = config();
    c.origin = "http://approve.example.org".into();
    assert!(s.deck_configure(&c).is_err());
    s.deck_configure(&config()).unwrap();
    refused(s.deck_present("x", 1, "x", "approve"), "disabled");
    assert!(s.deck_configure(&config()).is_err());
}

#[test]
fn request_author_choice_and_revision_are_explicit() {
    let mut s = Store::open_in_memory().unwrap();
    let a = Authenticator::es256();
    prepare(&mut s, &a);
    let mut r = request();
    r.author = "untrusted".into();
    r.revision = 2;
    refused(s.deck_put_request(&r), "unauthorized curator");
    refused(s.deck_put_request(&request()), "revision");
    refused(
        s.deck_present(&r.id, 1, "dGVzdA", "execute anything"),
        "undeclared",
    );
    refused(s.deck_present(&r.id, 2, "dGVzdA", "approve"), "stale");
    assert!(
        s.deck_put_request(&Request {
            choices: vec!["approve".into(), "approve".into()],
            ..request()
        })
        .is_err()
    );
    let g = s.intern(GRAPH).unwrap();
    let ts = crate::time::now_iso();
    let d = datum(&s, &r.id, "forPolicy", Value::Str("spoof".into()), &ts).unwrap();
    // Even exact matching actor/source strings are not an authority token.
    refused(
        s.transact_to_graph(&[d], &ts, Some("test-curator"), Some("decision-deck"), g),
        "protected writer",
    );
    assert_eq!(s.deck_request(&r.id).unwrap().policy, "test-only-policy");
}

#[test]
fn accepted_es256_and_eddsa_have_reverifiable_receipts_and_replay_refusal() {
    for a in [Authenticator::es256(), Authenticator::ed25519()] {
        let mut s = Store::open_in_memory().unwrap();
        prepare(&mut s, &a);
        let empty = crate::sparql::query(&s, &audit_query().template).unwrap();
        assert_eq!(empty.rows().len(), 0);
        let control = crate::sparql::query(&s, &format!("PREFIX a: <{DEFAULT_BASE_NS}> SELECT ?r FROM <{GRAPH}> WHERE {{ ?r a a:DecisionRequest }} LIMIT 2")).unwrap();
        assert_eq!(control.rows().len(), 1);
        let c = present(&mut s);
        let signature = signed(&a, &c);
        let receipt = s.deck_attest(&c.nonce, &signature.as_assertion()).unwrap();
        assert_eq!(receipt.challenge, c);
        let (recovered, tx) = s.deck_receipt(&c.nonce).unwrap().unwrap();
        assert!(tx > 0);
        assert_eq!(receipt, recovered);
        let collision = Request {
            id: receipt.decision.clone(),
            ..request()
        };
        assert!(
            s.deck_put_request(&collision).is_err(),
            "curation must retain prior decision audit facts"
        );
        assert!(s.deck_verify_receipt(&c.nonce).unwrap());
        let audit = audit_query();
        s.query_load(&audit, &crate::time::now_iso()).unwrap();
        let stored = s.query_get(&audit.name).unwrap().unwrap();
        let rows = crate::sparql::query(&s, &stored.template).unwrap();
        assert_eq!(rows.rows().len(), 1);
        println!(
            "{}: accepted one curated revision; audit rows={}; historical signature=true",
            a.scheme().tag(),
            rows.rows().len()
        );
        refused(
            s.deck_attest(&c.nonce, &signature.as_assertion()),
            "replayed",
        );
        println!(
            "same assertion: REFUSED replay; exact read-only receipt matches committed decision"
        );
        refused(
            s.deck_present(&c.request.id, 1, "dGVzdA", "reject"),
            "already decided",
        );
        let facts = s.current_facts_in_graph(s.intern(GRAPH).unwrap()).unwrap();
        let decision = s.lookup(&receipt.decision).unwrap().unwrap();
        assert_eq!(facts.iter().filter(|f| f.entity == decision).count(), 10);
        assert!(
            facts
                .iter()
                .filter(|f| f.entity == decision)
                .all(|f| f.tx == tx)
        );
        s.deck_set_enabled(false).unwrap();
        s.deck_revoke("dGVzdA").unwrap();
        assert!(s.deck_verify_receipt(&c.nonce).unwrap());
    }
}

#[test]
fn independent_challenges_cannot_double_decide_one_revision() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &a);
    let first = present(&mut s);
    let second = present(&mut s);
    assert_ne!(first.nonce, second.nonce);
    s.deck_attest(&first.nonce, &signed(&a, &first).as_assertion())
        .unwrap();
    assert!(
        s.deck_attest(&second.nonce, &signed(&a, &second).as_assertion())
            .is_err()
    );
    assert!(s.deck_receipt(&second.nonce).unwrap().is_none());
    let consumed: bool = s
        .conn
        .query_row(
            "SELECT consumed FROM deck_challenges WHERE nonce=?1",
            [second.nonce],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!consumed);
}

#[test]
fn signature_origin_rp_flags_and_content_negatives_preserve_positive_control() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &a);
    let c = present(&mut s);
    let make = || Ceremony::for_message(&c.message().unwrap());
    let mut variants = Vec::new();
    let mut v = make();
    v.origin = "https://other.example.org".into();
    variants.push((v, "client data"));
    let mut v = make();
    v.rp_id = "other.example.org".into();
    variants.push((v, "rpIdHash"));
    let mut v = make();
    v.flags = 1;
    variants.push((v, "verification"));
    let mut v = make();
    v.flags = 4;
    variants.push((v, "presence"));
    let mut v = make();
    v.cross_origin = true;
    variants.push((v, "cross-origin"));
    let mut v = make();
    v.challenge_message.push(1);
    variants.push((v, "challenge"));
    for (v, reason) in variants {
        refused(s.deck_attest(&c.nonce, &a.sign(&v).as_assertion()), reason);
        assert!(s.deck_receipt(&c.nonce).unwrap().is_none());
    }
    refused(
        s.deck_attest(
            &c.nonce,
            &signed(&Authenticator::es256(), &c).as_assertion(),
        ),
        "signature",
    );
    let mut bad = signed(&a, &c);
    bad.signature[0] ^= 1;
    refused(s.deck_attest(&c.nonce, &bad.as_assertion()), "signature");
    s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion())
        .unwrap();
}

#[test]
fn stale_expired_revoked_and_disabled_are_refused() {
    for case in ["stale", "expired", "revoked", "disabled", "audience"] {
        let a = Authenticator::es256();
        let mut s = Store::open_in_memory().unwrap();
        prepare(&mut s, &a);
        let c = present(&mut s);
        match case {
            "stale" => {
                let mut r = request();
                r.revision = 2;
                r.summary = "Changed proposition".into();
                s.deck_put_request(&r).unwrap();
            }
            "expired" => {
                let mut changed = c.clone();
                changed.expires_at = now().unwrap();
                s.conn
                    .execute(
                        "UPDATE deck_challenges SET data=?1 WHERE nonce=?2",
                        params![encode(&changed).unwrap(), c.nonce],
                    )
                    .unwrap();
            }
            "revoked" => {
                s.deck_revoke("dGVzdA").unwrap();
            }
            "disabled" => s.deck_set_enabled(false).unwrap(),
            "audience" => {
                s.conn
                    .execute(
                        "UPDATE store_identity SET store_id='urn:test:other-store' WHERE id=1",
                        [],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion())
                .is_err(),
            "{case}"
        );
        assert!(s.deck_receipt(&c.nonce).unwrap().is_none());
    }
}

#[test]
fn late_settlement_failure_rolls_back_graph_nonce_counter_and_receipt() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &a);
    let c = present(&mut s);
    let before = s
        .current_facts_in_graph(s.intern(GRAPH).unwrap())
        .unwrap()
        .len();
    drop(s.read_model_for(s.intern(GRAPH).unwrap()).unwrap());
    s.conn.execute_batch("CREATE TEMP TRIGGER fail_deck_tx BEFORE UPDATE OF tx ON deck_receipts BEGIN SELECT RAISE(ABORT,'late settlement failure'); END;").unwrap();
    refused(
        s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion()),
        "late settlement failure",
    );
    assert_eq!(
        s.current_facts_in_graph(s.intern(GRAPH).unwrap())
            .unwrap()
            .len(),
        before
    );
    assert!(s.deck_receipt(&c.nonce).unwrap().is_none());
    assert_eq!(s.deck_credential("dGVzdA").unwrap().1, 0);
    assert_eq!(
        crate::sparql::query(&s, &audit_query().template)
            .unwrap()
            .rows()
            .len(),
        0
    );
    s.conn.execute_batch("DROP TRIGGER fail_deck_tx").unwrap();
    s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion())
        .unwrap();
    assert_eq!(
        crate::sparql::query(&s, &audit_query().template)
            .unwrap()
            .rows()
            .len(),
        1
    );
}

#[test]
fn graph_store_cannot_replace_or_delete_protected_projection() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &a);
    let ts = crate::time::now_iso();
    assert!(crate::delete_rdf_graph(&mut s, &ts, Some("test-curator"), Some(GRAPH)).is_err());
    assert!(
        crate::replace_rdf_graph(
            &mut s,
            b"".as_slice(),
            oxrdfio::RdfFormat::NTriples,
            None,
            &ts,
            Some("test-curator"),
            Some("decision-deck"),
            Some(GRAPH)
        )
        .is_err()
    );
    assert!(s.registered_graph_id(GRAPH).unwrap().is_some());
    let c = present(&mut s);
    s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion())
        .unwrap();
    assert!(s.deck_verify_receipt(&c.nonce).unwrap());
}

#[test]
fn malformed_client_data_and_counter_regression_do_not_consume_challenge() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &a);
    let c = present(&mut s);
    let mut bad = signed(&a, &c);
    bad.client_data_json=br#"{"type":"webauthn.get","type":"webauthn.get","challenge":"x","origin":"https://approve.example.org"}"#.to_vec();
    refused(
        s.deck_attest(&c.nonce, &bad.as_assertion()),
        "duplicate field",
    );
    bad.client_data_json=br#"{"type":"webauthn.get","challenge":"x","origin":"https://approve.example.org","crossOrigin":"false"}"#.to_vec();
    assert!(s.deck_attest(&c.nonce, &bad.as_assertion()).is_err());
    s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion())
        .unwrap();
    let mut r = request();
    r.revision = 2;
    s.deck_put_request(&r).unwrap();
    let next = s.deck_present(&r.id, 2, "dGVzdA", "approve").unwrap();
    refused(
        s.deck_attest(&next.nonce, &signed(&a, &next).as_assertion()),
        "counter",
    );
    let mut advance = Ceremony::for_message(&next.message().unwrap());
    advance.sign_count = 8;
    s.deck_attest(&next.nonce, &a.sign(&advance).as_assertion())
        .unwrap();
    assert_eq!(s.deck_credential("dGVzdA").unwrap().1, 8);
}

#[test]
fn nested_transaction_is_refused_before_any_decision_write() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &a);
    s.conn.execute_batch("SAVEPOINT outer_fixture").unwrap();
    refused(
        s.deck_present("urn:test:curated-request", 1, "dGVzdA", "approve"),
        "independent writer",
    );
    s.conn
        .execute_batch("ROLLBACK TO outer_fixture; RELEASE outer_fixture")
        .unwrap();
    assert!(
        s.deck_present("urn:test:curated-request", 1, "dGVzdA", "approve")
            .is_ok()
    );
}

#[test]
fn failed_outer_savepoint_discards_provisional_term_memo() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    prepare(&mut s, &a);
    let result: Result<()> = s.deck_atomic("test-human", |store, _| {
        let term = store.intern("urn:test:tentative-deck-term")?;
        assert_eq!(store.lookup("urn:test:tentative-deck-term")?, Some(term));
        assert_eq!(store.resolve(term)?, "urn:test:tentative-deck-term");
        Err(refuse("fixture rolls back"))
    });
    refused(result, "fixture rolls back");
    assert_eq!(s.lookup("urn:test:tentative-deck-term").unwrap(), None);
    let fresh = s.intern("urn:test:committed-control").unwrap();
    assert_eq!(s.resolve(fresh).unwrap(), "urn:test:committed-control");
}

#[test]
fn durable_reopen_and_lost_response_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("deck.db");
    let a = Authenticator::es256();
    let c;
    {
        let mut s = Store::open(file.to_str().unwrap()).unwrap();
        prepare(&mut s, &a);
        c = present(&mut s);
        s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion())
            .unwrap();
    }
    let mut s = Store::open(file.to_str().unwrap()).unwrap();
    assert!(s.deck_receipt(&c.nonce).unwrap().is_some());
    assert!(s.deck_verify_receipt(&c.nonce).unwrap());
    refused(
        s.deck_attest(&c.nonce, &signed(&a, &c).as_assertion()),
        "replayed",
    );
    assert!(s.deck_receipt("unknown").unwrap().is_none());
}
