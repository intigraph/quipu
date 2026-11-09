//! Synthetic local-owner enrollment only; no production key or profile.
use super::*;
use crate::client_changeset::{ClientOverlay, ProposalIdentity};
use crate::{Datum, Op, Value};

const TS: &str = "2026-01-01T00:00:00Z";
const SHAPES: &str = "@prefix sh:<http://www.w3.org/ns/shacl#>. <urn:Shape> a sh:NodeShape; sh:targetSubjectsOf <urn:p>; sh:property [sh:path <urn:p>; sh:maxCount 1].";

fn put(store: &mut Store, subject: &str, predicate: &str, value: &str) {
    let row = Datum {
        entity: store.intern(subject).unwrap(),
        attribute: store.intern(predicate).unwrap(),
        value: Value::Str(value.into()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    store.transact(&[row], TS, None, Some("fixture")).unwrap();
}

fn fixture(store: &mut Store) -> (SignedChangeSet, ClientWriteGrant) {
    fixture_shapes(store, SHAPES)
}

fn fixture_shapes(store: &mut Store, shapes: &str) -> (SignedChangeSet, ClientWriteGrant) {
    store.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
    store.load_shapes("one", shapes, TS).unwrap();
    put(store, "urn:s", "urn:p", "base");
    let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[41; 32]).unwrap();
    let public_key = quipu_signing::public_key_hex(&key);
    let ns = crate::namespace::DEFAULT_BASE_NS;
    let entity = store.intern("urn:reg:synthetic").unwrap();
    let class = store.intern(&format!("{ns}VerifierRegistration")).unwrap();
    // Synthetic authoritative fixture: admission's actual current registry
    // reader is tested, not the separately governed real S3 issuance process.
    for (predicate, value) in [
        (crate::namespace::RDF_TYPE.to_string(), Value::Ref(class)),
        (format!("{ns}verifier"), Value::Str("synthetic".into())),
        (format!("{ns}publicKey"), Value::Str(public_key.clone())),
        (format!("{ns}trustTier"), Value::Str("agent".into())),
    ] {
        let attribute = store.intern(&predicate).unwrap();
        store.conn.execute("INSERT INTO facts(e,a,v,tx,valid_from,valid_to,op,g) VALUES(?1,?2,?3,?4,?5,NULL,1,0)",
            rusqlite::params![entity, attribute, value.to_bytes(), store.latest_tx_id().unwrap(), TS]).unwrap();
    }
    let base = base_journal(store, store.latest_tx_id().unwrap()).unwrap();
    let mut overlay = ClientOverlay::open(base, "urn:fork:synthetic".into()).unwrap();
    let mut proposed = overlay.read().clone();
    proposed.remove("<urn:s> <urn:p> \"base\"");
    proposed.insert("<urn:s> <urn:p> \"client\"".into());
    overlay.replace(proposed).unwrap();
    let signed = overlay
        .sign(
            &key,
            ProposalIdentity {
                key_id: "urn:reg:synthetic".into(),
                signer: "synthetic".into(),
                operation_id: "00000000-0000-4000-8000-000000000001".into(),
                issued_at: TS.into(),
                nonce: "11".repeat(16),
            },
        )
        .unwrap();
    let grant = ClientWriteGrant {
        id: "synthetic-local-owner".into(),
        revision: 1,
        key_id: signed.envelope.key_id.clone(),
        signer: signed.envelope.signer.clone(),
        public_key,
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        graph: "ROOT".into(),
        predicates: ["urn:p".into()].into(),
        valid_from: 0,
        valid_until: 4102444800,
        revoked: false,
    };
    (signed, grant)
}

fn state(store: &Store) -> (i64, usize, i64) {
    (
        store.latest_tx_id().unwrap(),
        store.current_fact_count(0).unwrap(),
        store
            .conn
            .query_row("SELECT COUNT(*) FROM client_changeset_receipts", [], |r| {
                r.get(0)
            })
            .unwrap(),
    )
}

fn resign(mut proposal: SignedChangeSet, changes: crate::changeset::ChangeSet) -> SignedChangeSet {
    let changes =
        crate::changeset::ChangeSet::new(changes.header, changes.deletes, changes.inserts).unwrap();
    proposal.body = changes.to_sparql_update();
    proposal.envelope.body_sha256 = digest(proposal.body.as_bytes());
    proposal.envelope.delta_sha256 = format!("sha256:{}", changes.digest());
    let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[41; 32]).unwrap();
    proposal.signature = quipu_signing::sign_hex(&key, &proposal.envelope.message().unwrap());
    proposal
        .verify(&quipu_signing::public_key_hex(&key))
        .unwrap();
    proposal
}

#[test]
fn validly_signed_extra_predicate_and_foreign_graph_refuse() {
    let mut store = Store::open_in_memory().unwrap();
    let (proposal, grant) = fixture(&mut store);
    store.apply_client_write_grant(&grant).unwrap();
    let before = state(&store);
    let mut changes = proposal.changeset().unwrap();
    changes.inserts.push(crate::changeset::Statement::new(
        "<urn:s> <urn:extra> \"x\"",
        None,
    ));
    let extra = resign(proposal.clone(), changes);
    assert!(admit(&mut store, &extra).is_err());
    assert_eq!(state(&store), before);
    let mut foreign = proposal.clone();
    let mut changes = foreign.changeset().unwrap();
    for row in changes.inserts.iter_mut().chain(&mut changes.deletes) {
        row.graph = Some("urn:foreign".into());
    }
    foreign.envelope.graph = "urn:foreign".into();
    foreign.envelope.scope_digest = scope_digest(&["urn:foreign".into()], &[]);
    let foreign = resign(foreign, changes);
    assert!(admit(&mut store, &foreign).is_err());
    assert_eq!(state(&store), before);
}

#[test]
fn no_grant_attestation_alone_refuses_without_any_write() {
    let mut store = Store::open_in_memory().unwrap();
    let (proposal, _) = fixture(&mut store);
    let before = state(&store);
    assert!(admit(&mut store, &proposal).is_err());
    assert_eq!(state(&store), before);
}

#[test]
fn actual_three_way_merge_receipt_and_identical_retry() {
    let mut store = Store::open_in_memory().unwrap();
    let (proposal, grant) = fixture(&mut store);
    store.apply_client_write_grant(&grant).unwrap();
    put(&mut store, "urn:other", "urn:q", "master");
    let done = admit(&mut store, &proposal).unwrap();
    assert_eq!(done.verdict, "accepted");
    let rows = side_statements(&store, &DiffSide::Main, None).unwrap();
    assert!(
        rows.iter()
            .any(|s| s.triple == "<urn:other> <urn:q> \"master\"")
    );
    assert!(
        rows.iter()
            .any(|s| s.triple == "<urn:s> <urn:p> \"client\"")
    );
    let before = state(&store);
    assert_eq!(admit(&mut store, &proposal).unwrap(), done);
    assert_eq!(state(&store), before);
}

#[test]
fn current_scope_window_revocation_and_registration_are_mandatory() {
    for mutant in 0..7 {
        let mut store = Store::open_in_memory().unwrap();
        let (proposal, mut grant) = fixture(&mut store);
        match mutant {
            0 => grant.predicates = ["urn:wrong".into()].into(),
            1 => grant.revoked = true,
            2 => grant.valid_until = 1,
            3 => grant.valid_from = 4000000000,
            4 => grant.key_id = "urn:reg:other".into(),
            5 => grant.signer = "other".into(),
            _ => grant.public_key = "00".repeat(32),
        }
        store.apply_client_write_grant(&grant).unwrap();
        let before = state(&store);
        assert!(
            admit(&mut store, &proposal).is_err(),
            "authority mutant {mutant}"
        );
        assert_eq!(state(&store), before);
    }
}

#[test]
fn conflict_and_corrupt_base_do_not_commit_anything() {
    let mut store = Store::open_in_memory().unwrap();
    let (proposal, grant) = fixture(&mut store);
    store.apply_client_write_grant(&grant).unwrap();
    // A different master value at the same functional slot cannot silently win.
    let e = store.lookup("urn:s").unwrap().unwrap();
    let a = store.lookup("urn:p").unwrap().unwrap();
    store
        .retract_triples(e, Some(a), None, "2026-01-02T00:00:00Z", None, false, None)
        .unwrap();
    put(&mut store, "urn:s", "urn:p", "master");
    let before = state(&store);
    assert!(admit(&mut store, &proposal).is_err());
    assert_eq!(state(&store), before);
    let mut wrong = proposal.clone();
    wrong.envelope.base_slice_hash = digest(b"invented base");
    assert!(admit(&mut store, &wrong).is_err());
    assert_eq!(state(&store), before);
}

#[test]
fn receipt_failure_rolls_back_real_merge_and_retry_succeeds() {
    let mut store = Store::open_in_memory().unwrap();
    let (proposal, grant) = fixture(&mut store);
    store.apply_client_write_grant(&grant).unwrap();
    store.conn.execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON client_changeset_receipts BEGIN SELECT RAISE(ABORT,'receipt failure'); END;").unwrap();
    let before = state(&store);
    assert!(
        admit(&mut store, &proposal)
            .unwrap_err()
            .to_string()
            .contains("receipt failure")
    );
    assert_eq!(state(&store), before);
    store
        .conn
        .execute_batch("DROP TRIGGER fail_receipt")
        .unwrap();
    admit(&mut store, &proposal).unwrap();
}

#[test]
fn durable_retry_after_reopen_and_changed_operation_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("master.db");
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    let (proposal, grant) = fixture(&mut store);
    store.apply_client_write_grant(&grant).unwrap();
    let done = admit(&mut store, &proposal).unwrap();
    drop(store);
    let mut reopened = Store::open(path.to_str().unwrap()).unwrap();
    let before = state(&reopened);
    assert_eq!(admit(&mut reopened, &proposal).unwrap(), done);
    let mut changed = proposal.clone();
    changed.envelope.nonce = "22".repeat(16);
    let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[41; 32]).unwrap();
    changed.signature = quipu_signing::sign_hex(&key, &changed.envelope.message().unwrap());
    assert!(admit(&mut reopened, &changed).is_err());
    assert_eq!(state(&reopened), before);
}

#[test]
fn shapes_drift_refuses_without_writes() {
    let mut store = Store::open_in_memory().unwrap();
    let (proposal, grant) = fixture(&mut store);
    store.apply_client_write_grant(&grant).unwrap();
    // Add a second constraint after signing: current shapes drift must refuse.
    store.load_shapes("other", "@prefix sh:<http://www.w3.org/ns/shacl#>. <urn:Required> a sh:NodeShape; sh:targetNode <urn:s>; sh:property [sh:path <urn:required>; sh:minCount 1].", TS).unwrap();
    let before = state(&store);
    assert!(admit(&mut store, &proposal).is_err());
    assert_eq!(state(&store), before);
}

#[test]
fn actual_merged_only_shacl_failure_rolls_back_facts_and_receipt() {
    let mut store = Store::open_in_memory().unwrap();
    let shapes = format!(
        "{SHAPES} <urn:Cross> a sh:NodeShape; sh:targetNode <urn:s>; sh:not [sh:property [sh:path <urn:p>; sh:hasValue \"client\"]; sh:property [sh:path <urn:q>; sh:hasValue \"master\"]]."
    );
    let (proposal, grant) = fixture_shapes(&mut store, &shapes);
    store.apply_client_write_grant(&grant).unwrap();
    put(&mut store, "urn:s", "urn:q", "master");
    let before = state(&store);
    assert!(admit(&mut store, &proposal).is_err());
    assert_eq!(state(&store), before);
}

#[test]
fn owner_grant_revision_and_transaction_boundaries_are_not_caller_authority() {
    let mut store = Store::open_in_memory().unwrap();
    let (proposal, mut grant) = fixture(&mut store);
    store.apply_client_write_grant(&grant).unwrap();
    assert!(store.apply_client_write_grant(&grant).is_err());
    grant.graph = "urn:other".into();
    grant.revision += 1;
    assert!(store.apply_client_write_grant(&grant).is_err());
    core::atomic(&mut store, |s| {
        assert!(admit(s, &proposal).is_err());
        assert!(s.apply_client_write_grant(&grant).is_err());
        Ok(())
    })
    .unwrap();
}
