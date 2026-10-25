//! Temporary stores and software authenticators only.
use super::*;
use crate::namespace::{DEFAULT_BASE_NS, RDF_TYPE};
use crate::verdict_schemes::webauthn_tests::{Authenticator, Ceremony, ORIGIN, RP_ID};

pub(super) const OUT: &str = "https://example.org/graph/answers";
const SIDE: &str = "https://example.org/graph/items";
const ID: &str = "dGVzdA";
const ITEM: &str = "https://example.org/item/1";
const STATUS: &str = "https://example.org/status";
pub(super) const SLOT: &str = "https://example.org/answer/1";
pub(super) const SAID: &str = "https://example.org/said";

fn signer(auth: &Authenticator) -> Signer {
    Signer {
        id: ID.into(),
        scheme: auth.scheme().tag().into(),
        cose_key: auth.cose_key(),
        origin: ORIGIN.into(),
        rp_id: RP_ID.into(),
        actor: "test-human".into(),
        graphs: vec![OUT.into()],
        expires_at: now().unwrap() + 3600,
    }
}

pub(super) fn ready(auth: &Authenticator) -> Store {
    let mut s = Store::open_in_memory().unwrap();
    s.attest_protect_graph(OUT).unwrap();
    s.attest_enroll_signer(&signer(auth)).unwrap();
    s.attest_set_enabled(true).unwrap();
    set(&mut s, SIDE, ITEM, STATUS, None, Value::Str("open".into()));
    s
}

/// Replace the single value of `p` on `e` in an ORDINARY graph.
fn set(s: &mut Store, graph: &str, e: &str, p: &str, old: Option<Value>, new: Value) {
    let g = s.graph_create(graph).unwrap();
    let (e, a) = (s.intern(e).unwrap(), s.intern(p).unwrap());
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
    s.transact_to_graph(&datums, &ts, None, None, g).unwrap();
}

pub(super) fn proposal(text: &str) -> Proposal {
    Proposal {
        signer: ID.into(),
        graph: OUT.into(),
        datums: vec![AttestedDatum {
            s: SLOT.into(),
            p: SAID.into(),
            o: TypedValue::Str(text.into()),
        }],
        expect: vec![
            Expect {
                graph: SIDE.into(),
                s: ITEM.into(),
                p: STATUS.into(),
                o: Some(TypedValue::Str("open".into())),
            },
            Expect {
                graph: OUT.into(),
                s: SLOT.into(),
                p: SAID.into(),
                o: None,
            },
        ],
    }
}

pub(super) fn sign(
    auth: &Authenticator,
    c: &Challenge,
    count: u32,
) -> crate::verdict_schemes::webauthn_tests::Signed {
    let mut ceremony = Ceremony::for_message(&c.message().unwrap());
    ceremony.sign_count = count;
    auth.sign(&ceremony)
}

/// Receipts, consumed challenges and facts in the answers graph.
pub(super) fn footprint(s: &Store) -> (i64, i64, usize) {
    let n = |sql: &str| -> i64 { s.conn.query_row(sql, [], |r| r.get(0)).unwrap() };
    let facts = match s.registered_graph_id(OUT).unwrap() {
        Some(g) => s.current_facts_in_graph(g).unwrap().len(),
        None => 0,
    };
    (
        n("SELECT COUNT(*) FROM attest_receipts"),
        n("SELECT COUNT(*) FROM attest_challenges WHERE consumed=1"),
        facts,
    )
}

pub(super) fn refused<T: std::fmt::Debug>(r: Result<T>, fragment: &str) {
    let e = r.unwrap_err().to_string();
    assert!(e.contains(fragment), "{e} lacks {fragment}");
}

/// The string value of `p` on `e` in the answers graph.
fn value(s: &Store, e: &str, p: &str) -> Option<Value> {
    let g = s.registered_graph_id(OUT).unwrap()?;
    let (e, a) = (s.lookup(e).unwrap()?, s.lookup(p).unwrap()?);
    s.current_facts_in_graph(g)
        .unwrap()
        .into_iter()
        .find(|f| f.entity == e && f.attribute == a)
        .map(|f| f.value)
}

#[test]
fn default_off_refuses_and_enrollment_is_validated() {
    let a = Authenticator::es256();
    let mut s = Store::open_in_memory().unwrap();
    s.attest_protect_graph(OUT).unwrap();
    s.attest_enroll_signer(&signer(&a)).unwrap();
    refused(s.attest_challenge(&proposal("x")), "disabled");
    let bad = [
        Signer {
            id: "ZmFrZQ".into(),
            origin: "http://deck.example.org".into(),
            ..signer(&a)
        },
        Signer {
            id: "ZmFrZQ".into(),
            graphs: Vec::new(),
            ..signer(&a)
        },
        Signer {
            id: "ZmFrZQ".into(),
            actor: String::new(),
            ..signer(&a)
        },
        Signer {
            id: "ZmFrZQ".into(),
            expires_at: 1,
            ..signer(&a)
        },
        Signer {
            id: "ZmFrZQ".into(),
            scheme: "webauthn-eddsa".into(),
            ..signer(&a)
        },
    ];
    for signer in bad {
        assert!(s.attest_enroll_signer(&signer).is_err(), "{signer:?}");
    }
    refused(s.attest_enroll_signer(&signer(&a)), "UNIQUE");
    refused(
        s.attest_protect_graph("urn:quipu:decision-deck"),
        "reserved",
    );
}

#[test]
fn an_attested_write_lands_with_server_provenance() {
    let a = Authenticator::es256();
    let mut s = ready(&a);
    let p = proposal("tonight");
    let c = s.attest_challenge(&p).unwrap();
    assert_eq!(c.domain, DOMAIN);
    let receipt = s
        .attest_commit(&c.nonce, &sign(&a, &c, 8).as_assertion())
        .unwrap();
    assert_eq!(value(&s, SLOT, SAID), Some(Value::Str("tonight".into())));
    let node = admission::receipt_iri(&c.nonce);
    let by = format!("{DEFAULT_BASE_NS}attestedBy");
    assert_eq!(
        value(&s, SLOT, &by),
        Some(Value::Ref(s.lookup(&node).unwrap().unwrap()))
    );
    let recorded = value(&s, &node, &format!("{DEFAULT_BASE_NS}attestProposal")).unwrap();
    let Value::Str(json) = recorded else {
        panic!("{recorded:?}")
    };
    assert_eq!(
        serde_json::from_str::<Proposal>(&json).unwrap(),
        p,
        "exact proposal"
    );
    assert_eq!(
        value(&s, &node, &format!("{DEFAULT_BASE_NS}attestContentHash")),
        Some(Value::Str(crate::share::sha256(&c.message().unwrap())))
    );
    assert!(s.attest_verify_receipt(&c.nonce).unwrap());
    let (recovered, tx) = s.attest_receipt(&c.nonce).unwrap().unwrap();
    assert_eq!(recovered, receipt);
    assert!(tx.is_some());
}

#[test]
fn ordinary_and_lookalike_writes_cannot_reach_an_attested_graph() {
    let a = Authenticator::es256();
    let mut s = ready(&a);
    let g = s.graph_create(OUT).unwrap();
    let (e, at) = (s.intern(SLOT).unwrap(), s.intern(SAID).unwrap());
    let ts = crate::time::now_iso();
    let d = Datum {
        entity: e,
        attribute: at,
        value: Value::Str("forged".into()),
        valid_from: ts.clone(),
        valid_to: None,
        op: Op::Assert,
    };
    refused(
        s.transact_to_graph(&[d], &ts, None, None, g),
        "attested writes only",
    );
    // A signer cannot propose server provenance either.
    for datum in [
        AttestedDatum {
            s: SLOT.into(),
            p: format!("{DEFAULT_BASE_NS}attestedBy"),
            o: TypedValue::Iri(format!("urn:quipu:attest:{}", "0".repeat(64))),
        },
        AttestedDatum {
            s: format!("urn:quipu:attest:{}", "0".repeat(64)),
            p: SAID.into(),
            o: TypedValue::Str("x".into()),
        },
        AttestedDatum {
            s: SLOT.into(),
            p: format!("{DEFAULT_BASE_NS}attestProposal"),
            o: TypedValue::Str("{}".into()),
        },
    ] {
        refused(
            s.attest_challenge(&Proposal {
                datums: vec![datum],
                ..proposal("x")
            }),
            "invalid proposal",
        );
    }
    assert_eq!(footprint(&s), (0, 0, 0));
}

#[test]
fn a_graph_off_the_list_or_unprotected_is_refused() {
    let a = Authenticator::es256();
    let mut s = ready(&a);
    let other = "https://example.org/graph/other";
    refused(
        s.attest_challenge(&Proposal {
            graph: other.into(),
            ..proposal("x")
        }),
        "attested-only",
    );
    s.attest_protect_graph(other).unwrap();
    refused(
        s.attest_challenge(&Proposal {
            graph: other.into(),
            ..proposal("x")
        }),
        "attested-only",
    );
}

#[test]
fn a_cross_graph_precondition_is_checked_at_commit_and_records_nothing() {
    let a = Authenticator::es256();
    let mut s = ready(&a);
    let c = s.attest_challenge(&proposal("tonight")).unwrap();
    set(
        &mut s,
        SIDE,
        ITEM,
        STATUS,
        Some(Value::Str("open".into())),
        Value::Str("closed".into()),
    );
    let before = footprint(&s);
    refused(
        s.attest_commit(&c.nonce, &sign(&a, &c, 8).as_assertion()),
        "precondition",
    );
    assert_eq!(footprint(&s), before);
    // Control: the same IRI with the expected value in ANOTHER graph does not
    // satisfy an expectation that names the item graph.
    set(
        &mut s,
        "https://example.org/graph/copy",
        ITEM,
        STATUS,
        None,
        Value::Str("open".into()),
    );
    refused(
        s.attest_commit(&c.nonce, &sign(&a, &c, 8).as_assertion()),
        "precondition",
    );
    assert_eq!(footprint(&s), before);
    // Nothing was consumed: once the item reads open again, it commits.
    set(
        &mut s,
        SIDE,
        ITEM,
        STATUS,
        Some(Value::Str("closed".into())),
        Value::Str("open".into()),
    );
    s.attest_commit(&c.nonce, &sign(&a, &c, 8).as_assertion())
        .unwrap();
}

#[test]
fn an_absence_expectation_admits_one_write_per_slot() {
    let a = Authenticator::es256();
    let mut s = ready(&a);
    let first = s.attest_challenge(&proposal("tonight")).unwrap();
    let second = s.attest_challenge(&proposal("weekend")).unwrap();
    s.attest_commit(&first.nonce, &sign(&a, &first, 8).as_assertion())
        .unwrap();
    let before = footprint(&s);
    refused(
        s.attest_commit(&second.nonce, &sign(&a, &second, 9).as_assertion()),
        "precondition",
    );
    assert_eq!(footprint(&s), before);
    assert_eq!(value(&s, SLOT, SAID), Some(Value::Str("tonight".into())));
}

#[test]
fn replay_forgery_and_revocation_record_nothing() {
    let a = Authenticator::es256();
    let mut s = ready(&a);
    let c = s.attest_challenge(&proposal("tonight")).unwrap();
    let before = footprint(&s);
    // A signature over different content.
    let forged = Challenge {
        proposal: proposal("weekend"),
        ..c.clone()
    };
    assert!(
        s.attest_commit(&c.nonce, &sign(&a, &forged, 8).as_assertion())
            .is_err()
    );
    // Another key.
    assert!(
        s.attest_commit(
            &c.nonce,
            &sign(&Authenticator::es256(), &c, 8).as_assertion()
        )
        .is_err()
    );
    assert_eq!(footprint(&s), before);
    // The genuine commit, then its replay.
    s.attest_commit(&c.nonce, &sign(&a, &c, 8).as_assertion())
        .unwrap();
    let after = footprint(&s);
    refused(
        s.attest_commit(&c.nonce, &sign(&a, &c, 9).as_assertion()),
        "replayed",
    );
    assert_eq!(footprint(&s), after);
    // Revocation between challenge and commit refuses the commit.
    let mut s = ready(&a);
    let c = s
        .attest_challenge(&Proposal {
            expect: Vec::new(),
            ..proposal("x")
        })
        .unwrap();
    assert!(s.attest_revoke_signer(ID).unwrap());
    refused(
        s.attest_commit(&c.nonce, &sign(&a, &c, 8).as_assertion()),
        "revoked",
    );
    assert_eq!(footprint(&s), (0, 0, 0));
    // Disabling refuses too, and receipts stay readable.
    s.attest_set_enabled(false).unwrap();
    refused(
        s.attest_commit(&c.nonce, &sign(&a, &c, 8).as_assertion()),
        "disabled",
    );
}

#[test]
fn a_proposal_is_bounded() {
    let a = Authenticator::es256();
    let mut s = ready(&a);
    let datum = AttestedDatum {
        s: SLOT.into(),
        p: SAID.into(),
        o: TypedValue::Str("x".into()),
    };
    let bad = [
        Proposal {
            datums: Vec::new(),
            ..proposal("x")
        },
        Proposal {
            datums: vec![datum.clone(); 65],
            ..proposal("x")
        },
        Proposal {
            datums: vec![AttestedDatum {
                s: "not an iri".into(),
                ..datum.clone()
            }],
            ..proposal("x")
        },
        Proposal {
            datums: vec![AttestedDatum {
                o: TypedValue::Str("x".repeat(4097)),
                ..datum.clone()
            }],
            ..proposal("x")
        },
        Proposal {
            expect: (0..17)
                .map(|i| Expect {
                    graph: SIDE.into(),
                    s: ITEM.into(),
                    p: format!("{STATUS}{i}"),
                    o: None,
                })
                .collect(),
            ..proposal("x")
        },
    ];
    for p in bad {
        refused(s.attest_challenge(&p), "invalid proposal");
    }
    // Controls: the limits are admitted.
    s.attest_challenge(&Proposal {
        datums: vec![datum; 64],
        ..proposal("x")
    })
    .unwrap();
}

/// A write-gate refusal at commit makes zero accepted mutation, yet keeps the
/// normal refusal audit: exactly one `write.refused` event survives the
/// rollback, attributed to the attested write, as for any refused write.
#[test]
fn a_gate_refusal_at_commit_records_nothing_but_keeps_its_audit() {
    let a = Authenticator::es256();
    let mut s = ready(&a);
    // An action-boundary deny policy: a Doc must carry an rdfs:label.
    s.governance_config_mut().enforce_on_write = true;
    let ns = DEFAULT_BASE_NS;
    let doc = "http://ex/Doc";
    let policy = "http://ex/P1";
    let rows = [
        (
            RDF_TYPE.to_string(),
            Value::Ref(s.intern(&format!("{ns}Policy")).unwrap()),
        ),
        (format!("{ns}targets"), Value::Str(doc.into())),
        (
            format!("{ns}claim"),
            Value::Str("ASK { $target <http://www.w3.org/2000/01/rdf-schema#label> ?l }".into()),
        ),
        (format!("{ns}boundary"), Value::Str("action".into())),
        (format!("{ns}effect"), Value::Str("deny".into())),
    ];
    let ts = crate::time::now_iso();
    let datums: Vec<Datum> = rows
        .into_iter()
        .map(|(p, v)| Datum {
            entity: s.intern(policy).unwrap(),
            attribute: s.intern(&p).unwrap(),
            value: v,
            valid_from: ts.clone(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    s.transact(&datums, &ts, None, None).unwrap();
    let events = |s: &Store| {
        s.events_after(0, 10_000, Some(&["write.refused".to_string()]), None)
            .unwrap()
    };
    assert!(events(&s).is_empty(), "control: no refusal before");
    let unlabelled = Proposal {
        datums: vec![AttestedDatum {
            s: "http://ex/d1".into(),
            p: RDF_TYPE.into(),
            o: TypedValue::Iri(doc.into()),
        }],
        expect: Vec::new(),
        ..proposal("x")
    };
    let c = s.attest_challenge(&unlabelled).unwrap();
    let before = footprint(&s);
    refused(
        s.attest_commit(&c.nonce, &sign(&a, &c, 8).as_assertion()),
        "policy",
    );
    assert_eq!(footprint(&s), before, "zero accepted mutation");
    let after = events(&s);
    assert_eq!(after.len(), 1, "the refusal is audited");
    let payload: serde_json::Value = serde_json::from_str(&after[0].payload).unwrap();
    assert_eq!(payload["gate"], "policy");
    assert_eq!(payload["graph"], OUT);
    assert_eq!(payload["source"], SOURCE);
    assert_eq!(payload["actor"], "test-human");
}

#[cfg(feature = "shacl")]
const NAME_SHAPE: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
<https://example.org/shape/Said> a sh:NodeShape ;
    sh:targetSubjectsOf <https://example.org/said> ;
    sh:property [ sh:path <https://example.org/said> ; sh:maxLength 8 ] .";

/// Post-state SHACL admission applies to attested writes exactly as to
/// ordinary ones: a violating write is refused with zero accepted mutation
/// and a `shacl` refusal event; a conforming one is admitted. Paired for
/// loaded shapes and for explicit write shapes.
#[cfg(feature = "shacl")]
#[test]
fn shacl_poststate_admission_applies_to_attested_writes() {
    let refusals = |s: &Store| {
        s.events_after(0, 10_000, Some(&["write.refused".to_string()]), None)
            .unwrap()
    };
    for explicit in [false, true] {
        let a = Authenticator::es256();
        let mut s = ready(&a);
        if !explicit {
            s.load_shapes("said", NAME_SHAPE, "2026-10-24T00:00:00Z")
                .unwrap();
        }
        let bad = s.attest_challenge(&proposal("much too long")).unwrap();
        let before = footprint(&s);
        let commit = |s: &mut Store, c: &Challenge, n: u32| {
            if explicit {
                s.with_write_shapes(Some(NAME_SHAPE), |s| {
                    s.attest_commit(&c.nonce, &sign(&a, c, n).as_assertion())
                })
            } else {
                s.attest_commit(&c.nonce, &sign(&a, c, n).as_assertion())
            }
        };
        assert!(commit(&mut s, &bad, 8).is_err(), "explicit={explicit}");
        assert_eq!(
            footprint(&s),
            before,
            "zero accepted mutation, explicit={explicit}"
        );
        let events = refusals(&s);
        assert_eq!(events.len(), 1, "explicit={explicit}");
        let payload: serde_json::Value = serde_json::from_str(&events[0].payload).unwrap();
        assert_eq!(payload["gate"], "shacl", "explicit={explicit}");
        assert_eq!(payload["source"], SOURCE);
        // Control: a conforming value is admitted under the same shapes.
        let good = s.attest_challenge(&proposal("short")).unwrap();
        commit(&mut s, &good, 9).unwrap();
        assert_eq!(value(&s, SLOT, SAID), Some(Value::Str("short".into())));
    }
}
