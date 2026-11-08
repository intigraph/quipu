//! Private database controls; no production producer or runtime registration.
use super::super::tests::{OUT, SAID, SLOT, footprint, proposal, ready, sign, signer};
use super::*;
use crate::namespace::DEFAULT_BASE_NS;
use crate::verdict_schemes::webauthn_tests::Authenticator;

fn prepare(s: &mut Store) {
    let graph = s.graph_create(OUT).unwrap();
    s.graph_ensure_registered(graph).unwrap();
    s.intern(SLOT).unwrap();
    s.intern(SAID).unwrap();
    install(s);
}

fn fixture() -> (Authenticator, Store, tempfile::TempDir, Sink) {
    let auth = Authenticator::es256();
    let mut store = ready(&auth);
    prepare(&mut store);
    let dir = tempfile::tempdir().unwrap();
    let sink = Sink::open(&dir.path().join("sink.db"));
    (auth, store, dir, sink)
}

fn challenge(s: &mut Store, value: &str) -> Challenge {
    let mut p = proposal(value);
    p.expect.clear();
    s.attest_challenge(&p).unwrap()
}

fn write(s: &mut Store, auth: &Authenticator, count: u32, cap: usize, fault: Fault) -> String {
    let c = challenge(s, "private-value-not-exported");
    let assertion = sign(auth, &c, count);
    let _armed = arm(s, &c.nonce, cap, fault);
    s.attest_commit(&c.nonce, &assertion.as_assertion())
        .unwrap();
    c.nonce
}

fn outbox(s: &Store) -> i64 {
    s.conn
        .query_row("SELECT COUNT(*) FROM fixture_census_outbox", [], |r| {
            r.get(0)
        })
        .unwrap()
}

#[test]
pub(super) fn lost_response_recovers_receipt_without_second_write() {
    let (auth, mut s, _dir, sink) = fixture();
    let nonce = write(&mut s, &auth, 1, LIMIT, Fault::None);
    let before = footprint(&s);
    let scan = reconcile(&s, &sink, false).unwrap();
    assert_eq!(
        scan,
        Scan {
            committed: 1,
            delivered: 1,
            ..Scan::default()
        }
    );
    assert_eq!(sink.count(), 1);
    assert_eq!(footprint(&s), before);
    assert!(
        s.attest_commit(
            &nonce,
            &sign(
                &auth,
                &s.attest_receipt(&nonce).unwrap().unwrap().0.challenge,
                2
            )
            .as_assertion()
        )
        .is_err()
    );
    assert_eq!(footprint(&s), before);
    assert_eq!(reconcile(&s, &sink, false).unwrap().delivered, 0);
    let payload: String = s
        .conn
        .query_row("SELECT payload FROM fixture_census_outbox", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(!payload.contains("private-value"));
    assert!(!payload.contains(SLOT));
}

#[test]
pub(super) fn authority_refusal_has_no_receipt_or_committed_bucket() {
    let (auth, mut s, _dir, sink) = fixture();
    let c = challenge(&mut s, "refused");
    s.governance_config_mut().enforce_authority = true;
    s.set_principal_chain(vec!["nobody".into()]);
    let before = footprint(&s);
    let _armed = arm(&s, &c.nonce, LIMIT, Fault::None);
    assert!(
        s.attest_commit(&c.nonce, &sign(&auth, &c, 1).as_assertion())
            .is_err()
    );
    assert_eq!(footprint(&s), before);
    assert_eq!(outbox(&s), 0);
    assert_eq!(reconcile(&s, &sink, false).unwrap(), Scan::default());
}

/// Exercise the native atomic primitive's receipt-only branch. The public
/// attested route always writes provenance RDF and does not expose this branch.
fn primitive(s: &mut Store, auth: &Authenticator, c: &Challenge, rollback: bool) -> Result<()> {
    let signed = sign(auth, c, 1);
    let key = signer(auth);
    let receipt = Receipt {
        challenge: c.clone(),
        content_hash: crate::share::sha256(&c.message()?),
        cose_key: key.cose_key,
        scheme: key.scheme,
        authenticator_data: signed.as_assertion().authenticator_data.to_vec(),
        client_data_json: signed.as_assertion().client_data_json.to_vec(),
        signature: signed.as_assertion().signature.to_vec(),
        recorded_at: now()?,
    };
    s.attest_atomic(|store, ts| {
        store.conn.execute(
            "INSERT INTO attest_receipts(nonce,data) VALUES(?1,?2)",
            params![c.nonce, encode(&receipt)?],
        )?;
        assert_eq!(
            store.conn.execute(
                "UPDATE attest_challenges SET consumed=1 WHERE nonce=?1 AND consumed=0",
                [&c.nonce]
            )?,
            1
        );
        store.conn.execute(
            "UPDATE attest_signers SET sign_count=1 WHERE id=?1",
            [&c.proposal.signer],
        )?;
        if !store.attest_verify_receipt(&c.nonce)? {
            return Err(refuse_capture());
        }
        if rollback {
            // The tap classifies first; the native graph-authority gate then
            // refuses and rolls back both receipt and nested outbox savepoint.
            let datum = Datum {
                entity: store.intern(SLOT)?,
                attribute: store.intern(SAID)?,
                value: Value::Str("x".into()),
                valid_from: ts.into(),
                valid_to: None,
                op: Op::Assert,
            };
            Ok(((), Some((OUT.into(), "fixture".into(), vec![datum]))))
        } else {
            Ok(((), None))
        }
    })
}

#[test]
pub(super) fn tentative_classification_rolls_back_with_outer_write() {
    let (auth, mut s, _dir, sink) = fixture();
    let c = challenge(&mut s, "tentative");
    s.governance_config_mut().enforce_authority = true;
    s.set_principal_chain(vec!["nobody".into()]);
    let before = footprint(&s);
    let _armed = arm(&s, &c.nonce, LIMIT, Fault::None);
    assert!(primitive(&mut s, &auth, &c, true).is_err());
    assert_eq!(LOST.with(Cell::get), 0, "classification itself succeeded");
    assert_eq!(
        EARLY_EXPORTS.with(Cell::get),
        0,
        "no tentative observation may escape"
    );
    assert_eq!(outbox(&s), 0);
    assert_eq!(footprint(&s), before);
    assert_eq!(reconcile(&s, &sink, false).unwrap(), Scan::default());
}

#[test]
pub(super) fn receipt_only_settlement_is_not_a_graph_commit() {
    let (auth, mut s, _dir, sink) = fixture();
    let c = challenge(&mut s, "receipt-only");
    let before = s.latest_tx_id().unwrap();
    let facts = footprint(&s).2;
    let _armed = arm(&s, &c.nonce, LIMIT, Fault::None);
    primitive(&mut s, &auth, &c, false).unwrap();
    assert_eq!(s.latest_tx_id().unwrap(), before);
    assert_eq!(footprint(&s), (1, 1, facts));
    assert_eq!(s.attest_receipt(&c.nonce).unwrap().unwrap().1, None);
    assert_eq!(
        reconcile(&s, &sink, false).unwrap(),
        Scan {
            no_data: 1,
            delivered: 1,
            ..Scan::default()
        }
    );
}

#[test]
pub(super) fn one_coalesced_hint_drains_three_receipts_once() {
    let (auth, mut s, _dir, sink) = fixture();
    for count in 1..=3 {
        write(&mut s, &auth, count, LIMIT, Fault::None);
    }
    assert_eq!(
        reconcile(&s, &sink, false).unwrap(),
        Scan {
            committed: 3,
            delivered: 3,
            ..Scan::default()
        }
    );
    assert_eq!(sink.count(), 3);
    assert_eq!(reconcile(&s, &sink, false).unwrap().delivered, 0);
    println!(
        "fixture: 3 durable native receipts, 1 drain hint, 3 unique exports, repeat drain 0 exports"
    );
}

#[test]
pub(super) fn close_reopen_and_lost_ack_preserve_durable_deduplication() {
    let auth = Authenticator::es256();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("writer.db");
    let sink_path = dir.path().join("sink.db");
    let mut s = Store::open(path.to_str().unwrap()).unwrap();
    s.set_base_ns(DEFAULT_BASE_NS.to_string());
    s.attest_protect_graph(OUT).unwrap();
    s.attest_enroll_signer(&signer(&auth)).unwrap();
    s.attest_set_enabled(true).unwrap();
    prepare(&mut s);
    write(&mut s, &auth, 1, LIMIT, Fault::None);
    drop(s);
    let s = Store::open(path.to_str().unwrap()).unwrap();
    let sink = Sink::open(&sink_path);
    assert_eq!(reconcile(&s, &sink, true).unwrap().delivered, 1);
    assert_eq!(sink.count(), 1);
    drop(sink);
    drop(s);
    let s = Store::open(path.to_str().unwrap()).unwrap();
    let sink = Sink::open(&sink_path);
    assert_eq!(reconcile(&s, &sink, false).unwrap().delivered, 1);
    assert_eq!(sink.count(), 1, "lost ack retries the same durable key");
    assert_eq!(reconcile(&s, &sink, false).unwrap().delivered, 0);
}

#[test]
pub(super) fn capacity_and_nonfatal_sql_failure_preserve_writer_results_and_report_gaps() {
    for (cap, fault) in [(0, Fault::None), (LIMIT, Fault::Sql)] {
        let (auth, mut s, _dir, sink) = fixture();
        let before = footprint(&s);
        write(&mut s, &auth, 1, cap, fault);
        assert_eq!(LOST.with(Cell::get), 1);
        assert_eq!(footprint(&s).0, before.0 + 1);
        assert_eq!(footprint(&s).1, before.1 + 1);
        assert!(footprint(&s).2 > before.2);
        assert_eq!(outbox(&s), 0);
        assert_eq!(
            reconcile(&s, &sink, false).unwrap(),
            Scan {
                gaps: 1,
                ..Scan::default()
            }
        );
    }
}

#[test]
fn busy_and_poisoned_sink_preserve_native_write_and_retryable_outbox() {
    let (auth, mut s, dir, sink) = fixture();
    sink.bounded.fixture_busy(|| {
        write(&mut s, &auth, 1, LIMIT, Fault::None);
        assert_eq!(reconcile(&s, &sink, false).unwrap().delivered, 0);
    });
    assert_eq!(footprint(&s).0, 1);
    assert_eq!(reconcile(&s, &sink, false).unwrap().delivered, 1);
    sink.bounded.fixture_poison();
    write(&mut s, &auth, 2, LIMIT, Fault::None);
    assert_eq!(reconcile(&s, &sink, false).unwrap().delivered, 0);
    assert_eq!(footprint(&s).0, 2);
    let fresh = Sink::open(&dir.path().join("sink.db"));
    assert_eq!(reconcile(&s, &fresh, false).unwrap().delivered, 1);
    assert_eq!(fresh.count(), 2);
}

#[test]
fn unarmed_native_writer_has_no_outbox_capture() {
    let (auth, mut s, _dir, sink) = fixture();
    let c = challenge(&mut s, "unarmed");
    s.attest_commit(&c.nonce, &sign(&auth, &c, 1).as_assertion())
        .unwrap();
    assert_eq!(outbox(&s), 0);
    assert_eq!(reconcile(&s, &sink, false).unwrap().gaps, 1);
}

#[test]
fn invalid_capture_binding_is_a_gap_and_tampered_receipt_cannot_export() {
    for field in ["store", "epoch", "base", "profile"] {
        let (auth, mut s, _dir, sink) = fixture();
        let c = challenge(&mut s, "bound");
        let _armed = arm(&s, &c.nonce, LIMIT, Fault::None);
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            let capture = active.as_mut().unwrap();
            match field {
                "store" => capture.store_id.push('x'),
                "epoch" => capture.epoch.push('x'),
                "base" => capture.base += 1,
                "profile" => capture.profile.push('x'),
                _ => unreachable!(),
            }
        });
        s.attest_commit(&c.nonce, &sign(&auth, &c, 1).as_assertion())
            .unwrap();
        assert_eq!(LOST.with(Cell::get), 1);
        assert_eq!(reconcile(&s, &sink, false).unwrap().gaps, 1);
        assert_eq!(sink.count(), 0);
    }
    let (auth, mut s, _dir, sink) = fixture();
    let nonce = write(&mut s, &auth, 1, LIMIT, Fault::None);
    let (mut receipt, _) = s.attest_receipt(&nonce).unwrap().unwrap();
    receipt.signature[0] ^= 1;
    s.conn
        .execute(
            "UPDATE attest_receipts SET data=?1 WHERE nonce=?2",
            params![encode(&receipt).unwrap(), nonce],
        )
        .unwrap();
    assert!(reconcile(&s, &sink, false).is_err());
    assert_eq!(sink.count(), 0);
}
