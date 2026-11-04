//! Actual native signed cut, catch-up and convergence.
use super::*;

#[test]
fn signed_cut_tail_private_overlay_admission_receipt_and_replica_convergence() {
    use crate::applier::{Applier, BootstrapTrust, Outcome, Source};
    use crate::asof_share;
    use crate::client_changeset::BaseJournal;
    use crate::replicate_source::{Session, StreamRequest};
    use crate::signing::SigningIdentity;
    use serde_json::{Value as Json, json};

    let mut f = fixture();
    let source_key = std::sync::Arc::new(SigningIdentity::new(
        ring::signature::Ed25519KeyPair::from_seed_unchecked(&[23; 32]).unwrap(),
        "synthetic-source",
    ));
    let spool = tempfile::tempdir().unwrap();
    let cut = asof_share::cut(
        &f.store,
        &asof_share::Request {
            graphs: vec![GRAPH.into()],
            context_graphs: vec![],
            tx: Some(f.overlay.base().base_tx()),
            expected_store_id: f.store.store_id().unwrap(),
        },
        spool.path(),
        &asof_share::Limits {
            disk_floor_bytes: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let records: Vec<Json> = std::fs::read_to_string(&cut.spool)
        .unwrap()
        .lines()
        .flat_map(|line| {
            serde_json::from_str::<Json>(line).unwrap()["records"]
                .as_array()
                .unwrap()
                .clone()
        })
        .collect();
    let signature = asof_share::sign_manifest(&cut.manifest, &source_key).unwrap();
    let mut replica = Store::open_in_memory().unwrap();
    let mut applier = Applier::open(&replica).unwrap();
    applier
        .require_signed_bootstrap(BootstrapTrust {
            public_key_hex: source_key.public_key_hex(),
            source: Source {
                store_id: cut.manifest.source.store_id.clone(),
                log_epoch: cut.manifest.source.log_epoch.clone(),
            },
            scope_digest: cut.manifest.scope_digest.clone(),
            policy_digest: cut.manifest.policy_digest.clone(),
            manifest_sha256: cut.manifest_sha256.clone(),
        })
        .unwrap();
    let frame = json!({"manifest":cut.manifest,"records":records,"manifest_signature":signature});
    assert!(matches!(
        applier
            .apply_frame(&mut replica, "quipu.asof", &frame.to_string(), 1)
            .unwrap(),
        Outcome::Bootstrapped { .. }
    ));
    println!(
        "SIGNED_CUT graph={GRAPH} source_tx={}",
        applier.progress().unwrap().source_tx
    );
    let id = replica.registered_graph_id(GRAPH).unwrap().unwrap();
    let local_rows = base::rows(&replica, id, replica.latest_tx_id().unwrap()).unwrap();
    let pinned = f.overlay.base();
    let base = BaseJournal::new(
        pinned.store_id.clone(),
        pinned.log_epoch.clone(),
        applier.progress().unwrap().source_tx,
        GRAPH.into(),
        pinned.shapes_hash.clone(),
        local_rows,
    )
    .unwrap();
    assert_eq!(base.slice_hash().unwrap(), pinned.slice_hash().unwrap());
    let mut client = ClientOverlay::open(base, "urn:fork:synthetic-client".into()).unwrap();
    client.replace(f.overlay.read().clone()).unwrap();
    let original_journal = client.base().clone();
    let original_proposal = client.read().clone();
    write(
        &mut f.store,
        "<urn:item> <urn:p:note> \"master while offline\" .",
        "",
        T1,
    );
    let (mut stream, open) = Session::open(
        &f.store,
        &StreamRequest {
            graphs: vec![GRAPH.into()],
            context_graphs: vec![],
            since: client.base().base_tx(),
            expected_store_id: client.base().store_id().into(),
        },
        "synthetic-client-session".into(),
        source_key.clone(),
    )
    .unwrap();
    applier.trust_stream(&source_key.public_key_hex(), &cut.manifest.scope_digest);
    assert!(matches!(
        applier
            .apply_frame(&mut replica, open.event, &open.data, 2)
            .unwrap(),
        Outcome::SessionOpened { .. }
    ));
    let (frames, _) = stream.poll(&f.store, 10, None).unwrap();
    for frame in frames {
        let outcome = applier
            .apply_frame(&mut replica, frame.event, &frame.data, 3)
            .unwrap();
        assert!(!matches!(
            outcome,
            Outcome::Refused { .. } | Outcome::Stopped { .. } | Outcome::Rebootstrap { .. }
        ));
    }
    assert_eq!(client.base(), &original_journal);
    assert_eq!(client.read(), &original_proposal);
    println!(
        "TAIL_APPLIED frozen_base_retained={} local_proposal_retained={}",
        client.base() == &original_journal,
        client.read() == &original_proposal
    );
    assert!(
        render(
            &graph(&base::rows(&replica, id, replica.latest_tx_id().unwrap()).unwrap()).unwrap()
        )
        .contains("master while offline")
    );
    f.overlay = client;
    let proposal = proposal(&f);
    let Admission::Accepted { receipt, .. } = f.store.admit_client_changeset(&proposal).unwrap()
    else {
        panic!("expected merge receipt")
    };
    println!(
        "ADMISSION {}",
        json!({"operation_id":receipt.operation_id,
        "accepted_tx":receipt.accepted_tx,"verdict":receipt.verdict})
    );
    let accepted_tx = f.store.latest_tx_id().unwrap();
    let Admission::Accepted {
        receipt: retry,
        replayed,
    } = f.store.admit_client_changeset(&proposal).unwrap()
    else {
        panic!("expected recovered receipt")
    };
    assert_eq!(retry, receipt);
    assert!(replayed);
    assert_eq!(f.store.latest_tx_id().unwrap(), accepted_tx);
    println!(
        "IDENTICAL_RETRY replayed={replayed} additional_transactions={}",
        f.store.latest_tx_id().unwrap() - accepted_tx
    );
    let mut invalid = proposal.clone();
    invalid.signature = "00".repeat(64);
    let error = f.store.admit_client_changeset(&invalid).unwrap_err();
    assert_eq!(f.store.latest_tx_id().unwrap(), accepted_tx);
    println!("BAD_SIGNATURE {error}");
    let (frames, _) = stream.poll(&f.store, 10, None).unwrap();
    for frame in frames {
        let outcome = applier
            .apply_frame(&mut replica, frame.event, &frame.data, 4)
            .unwrap();
        assert!(!matches!(
            outcome,
            Outcome::Refused { .. } | Outcome::Stopped { .. } | Outcome::Rebootstrap { .. }
        ));
    }
    assert!(applier.progress().unwrap().source_tx >= receipt.accepted_tx);
    let replica_graph =
        graph(&base::rows(&replica, id, replica.latest_tx_id().unwrap()).unwrap()).unwrap();
    assert_eq!(render(&replica_graph), state(&f.store).1);
    println!("REPLICA_CONVERGED\n{}", render(&replica_graph));
}
