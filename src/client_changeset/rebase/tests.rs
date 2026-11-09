use super::*;

const SHAPES: &str =
    "@prefix sh: <http://www.w3.org/ns/shacl#> . [] sh:path <urn:p>; sh:maxCount 1 .";
const BASE: &str = "<urn:s> <urn:p> \"base\"";
const LOCAL: &str = "<urn:s> <urn:p> \"local\"";
const NOTE: &str = "<urn:s> <urn:note> \"upstream\"";

fn journal(tx: i64, triples: &[&str]) -> BaseJournal {
    BaseJournal::new(
        "urn:store:rebase".into(),
        "urn:epoch:rebase".into(),
        tx,
        "ROOT".into(),
        digest(SHAPES.as_bytes()),
        triples.iter().map(|t| BaseRow {
            triple: (*t).into(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
        }),
    )
    .unwrap()
}

fn identity(n: u8) -> ProposalIdentity {
    ProposalIdentity {
        key_id: "urn:registration:rebase".into(),
        signer: "fixture".into(),
        operation_id: format!("00000000-0000-4000-8000-{n:012}"),
        issued_at: "2026-01-02T00:00:00Z".into(),
        nonce: format!("{n:02x}").repeat(16),
    }
}

fn local() -> ClientOverlay {
    let mut overlay = ClientOverlay::open(journal(3, &[BASE]), "urn:fork:rebase".into()).unwrap();
    overlay.replace([LOCAL.into()]).unwrap();
    overlay
}

fn key() -> ring::signature::Ed25519KeyPair {
    ring::signature::Ed25519KeyPair::from_seed_unchecked(&[9; 32]).unwrap()
}

#[test]
fn rebase_preserves_old_signed_operation_and_proposes_only_remaining_delta() {
    let overlay = local();
    let key = key();
    let pk = quipu_signing::public_key_hex(&key);
    let previous = overlay.sign(&key, identity(1)).unwrap();
    let before = serde_json::to_vec(&(&overlay, &previous)).unwrap();
    let current = journal(4, &[BASE, NOTE]);
    let RebaseOutcome::Prepared { rebased } = overlay
        .rebase(&previous, &pk, &current, SHAPES, identity(2))
        .unwrap()
    else {
        panic!("independent upstream addition must rebase");
    };
    assert_eq!(rebased.previous, previous);
    assert_eq!(before, serde_json::to_vec(&(&overlay, &previous)).unwrap());
    assert_eq!(rebased.overlay.base(), &current);
    assert_eq!(rebased.overlay.fork_id(), overlay.fork_id());
    assert_eq!(rebased.overlay.read(), &[LOCAL.into(), NOTE.into()].into());
    let cs = rebased.overlay.changeset().unwrap();
    assert_eq!(cs.deletes, vec![Statement::new(BASE, None)]);
    assert_eq!(cs.inserts, vec![Statement::new(LOCAL, None)]);
    assert_eq!(rebased.prepared.envelope.base_tx, 4);
    assert_eq!(
        rebased.prepared.envelope.base_slice_hash,
        current.slice_hash().unwrap()
    );
    assert_eq!(
        rebased.supersession.old_fingerprint,
        previous.fingerprint().unwrap()
    );
    assert_eq!(
        rebased.supersession.new_fingerprint,
        digest(&rebased.prepared.message().unwrap())
    );
    assert_eq!(
        rebased.supersession.old_operation_id,
        identity(1).operation_id
    );
    assert_eq!(
        rebased.supersession.new_operation_id,
        identity(2).operation_id
    );
    assert_ne!(
        rebased.supersession.old_fingerprint,
        rebased.supersession.new_fingerprint
    );
    let signed = rebased.overlay.sign(&key, identity(2)).unwrap();
    signed.verify(&pk).unwrap();
    assert_eq!(signed.envelope, rebased.prepared.envelope);
    assert_eq!(signed.body, rebased.prepared.body);
    let mut old_signature = signed;
    old_signature.signature = previous.signature.clone();
    assert!(old_signature.verify(&pk).is_err());
    let restored: RebasedChangeSet =
        serde_json::from_slice(&serde_json::to_vec(&rebased).unwrap()).unwrap();
    assert_eq!(restored.previous, previous);
    assert_eq!(restored.overlay.base(), &current);
}

#[test]
fn rebase_conflict_retains_old_state_without_preparing_or_superseding() {
    let overlay = local();
    let previous = overlay.sign(&key(), identity(1)).unwrap();
    let before = serde_json::to_vec(&(&overlay, &previous)).unwrap();
    for triples in [vec!["<urn:s> <urn:p> \"master\"", NOTE], vec![NOTE]] {
        let result = overlay
            .rebase(
                &previous,
                &quipu_signing::public_key_hex(&key()),
                &journal(4, &triples),
                SHAPES,
                identity(2),
            )
            .unwrap();
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["outcome"], "conflicts");
        assert!(value.get("rebased").is_none());
        let RebaseOutcome::Conflicts { records } = result else {
            panic!("expected conflict")
        };
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].theirs, vec!["\"local\""]);
        assert_eq!(before, serde_json::to_vec(&(&overlay, &previous)).unwrap());
    }
}

#[test]
fn rebase_refuses_old_operation_substitution_tampering_and_reused_identity() {
    let overlay = local();
    let key = key();
    let pk = quipu_signing::public_key_hex(&key);
    let previous = overlay.sign(&key, identity(1)).unwrap();
    let current = journal(4, &[BASE, NOTE]);
    let mut same_id = identity(2);
    same_id.operation_id = previous.envelope.operation_id.clone();
    let mut same_nonce = identity(2);
    same_nonce.nonce = previous.envelope.nonce.clone();
    for id in [same_id, same_nonce] {
        assert!(
            overlay
                .rebase(&previous, &pk, &current, SHAPES, id)
                .is_err()
        );
    }
    let mut changed = overlay.clone();
    changed
        .replace(["<urn:s> <urn:p> \"other\"".into()])
        .unwrap();
    let wrong_proposal = changed.sign(&key, identity(1)).unwrap();
    let mut bad_signature = previous.clone();
    bad_signature.signature = "00".repeat(64);
    let mut wrong_fork =
        ClientOverlay::open(overlay.base().clone(), "urn:fork:other".into()).unwrap();
    wrong_fork.replace(overlay.read().iter().cloned()).unwrap();
    let wrong_fork = wrong_fork.sign(&key, identity(1)).unwrap();
    for old in [wrong_proposal, bad_signature, wrong_fork] {
        assert!(
            overlay
                .rebase(&old, &pk, &current, SHAPES, identity(2))
                .is_err()
        );
    }
}

#[test]
fn rebase_refuses_foreign_or_stale_base_and_temporal_scope() {
    let overlay = local();
    let previous = overlay.sign(&key(), identity(1)).unwrap();
    let pk = quipu_signing::public_key_hex(&key());
    let current = journal(4, &[BASE, NOTE]);
    for field in ["store_id", "log_epoch", "graph", "shapes_hash", "base_tx"] {
        let mut value = serde_json::to_value(&current).unwrap();
        value[field] = if field == "base_tx" {
            serde_json::json!(2)
        } else {
            serde_json::json!("urn:foreign")
        };
        let foreign: BaseJournal = serde_json::from_value(value).unwrap();
        assert!(
            overlay
                .rebase(&previous, &pk, &foreign, SHAPES, identity(2))
                .is_err(),
            "{field}"
        );
    }
    assert!(
        overlay
            .rebase(
                &previous,
                &pk,
                &journal(3, &[BASE, NOTE]),
                SHAPES,
                identity(2)
            )
            .is_err()
    );
    assert!(
        overlay
            .rebase(&previous, &pk, &current, "", identity(2))
            .is_err()
    );
    let mut bounded = current;
    let mut row = bounded.rows.pop_first().unwrap();
    row.valid_to = Some("2026-02-01T00:00:00Z".into());
    bounded.rows.insert(row);
    assert!(
        overlay
            .rebase(&previous, &pk, &bounded, SHAPES, identity(2))
            .is_err()
    );
}
