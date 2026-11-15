//! Actual share transport retains evidence; only explicit native admission asserts it.
use super::*;
use crate::share::{SHARE_PAYLOAD_MAX_BYTES, ShareDestination, ShareOptions, share_payload};
use crate::share_import::{ShareImportRequest, import_share, verify_share};
use crate::transaction_auth::{Identity, with_identity};

pub(super) fn importer() -> Identity {
    Identity {
        principal: "urn:carrier-importer".into(),
        credential_id: Some("carrier-test-bearer".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}

pub(super) fn request(proofs: &[Proof]) -> ShareImportRequest {
    // A relay's ordinary graph is empty. Its store identity is intentionally
    // NOT the event source, and producer/actor text cannot rewrite that origin.
    let relay = Store::open_in_memory().unwrap();
    let payload = share_payload(
        &relay,
        &ShareOptions {
            no_shapes: true,
            queries: Some(Vec::new()),
            destination: ShareDestination::Internal,
            fluent_proofs: proofs
                .iter()
                .map(|p| serde_json::to_string(p).unwrap())
                .collect(),
            ..Default::default()
        },
        SHARE_PAYLOAD_MAX_BYTES,
    )
    .unwrap();
    if let Some(p) = proofs.first() {
        assert_ne!(payload.manifest.store_id, p.body.source_store);
    }
    serde_json::from_value(serde_json::json!({
        "manifest": payload.manifest,
        "export_ntriples": payload.files["export.nt"],
        "shapes_turtle": payload.files["shapes.ttl"],
        "source": "urn:carrier-relay",
        "actor": "urn:forged-source-actor"
    }))
    .unwrap()
}

pub(super) fn proof(graph: &str) -> Proof {
    let (source, g) = super::tests::loaded_event(graph, "urn:carried-event");
    seal(
        capture(
            &source,
            g,
            "urn:fluent",
            "urn:carried-event",
            &source.store_id().unwrap(),
        )
        .unwrap(),
        "urn:source-witness",
        &super::tests::signer(9),
    )
    .unwrap()
}

#[test]
fn carrier_seals_exact_bytes_and_legacy_absence_remains_absent() {
    let p = proof(crate::schema::ROOT_GRAPH_IRI);
    let mut r = request(&[p]);
    verify_share(&r).unwrap();
    r.manifest.fluent_proofs[0].push(' ');
    assert!(
        verify_share(&r).is_err(),
        "even semantically equal JSON changes share identity"
    );
    r.manifest.fluent_proofs.clear();
    assert!(
        verify_share(&r).is_err(),
        "stripping evidence must change identity"
    );
    let empty = request(&[]);
    assert!(
        serde_json::to_value(&empty.manifest)
            .unwrap()
            .get("fluent_proofs")
            .is_none()
    );
    verify_share(&empty).unwrap();
    let mut too_many = empty;
    too_many.manifest.fluent_proofs = vec!["{}".into(); 65];
    assert!(verify_share(&too_many).is_err());
}

#[test]
fn transport_actor_and_bundle_identity_never_admit_origin_but_explicit_native_batch_does() {
    for graph in [crate::schema::ROOT_GRAPH_IRI, "urn:graph-a", "urn:graph-b"] {
        let p = proof(graph);
        let r = request(std::slice::from_ref(&p));
        let (mut receiver, g) = super::tests::loaded_shapes(graph);
        receiver.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
        import_share(
            &mut receiver,
            &r,
            "2026-11-08T00:00:00Z",
            Some("urn:transport-importer"),
        )
        .unwrap();
        let pending = crate::share_fluent::imported_proofs(&receiver, &r.manifest.share_id)
            .unwrap()
            .unwrap();
        assert_eq!(pending, r.manifest.fluent_proofs);
        assert!(receiver.lookup("urn:carried-event").unwrap().is_none());
        let before = receiver.latest_tx_id().unwrap();
        assert!(
            with_identity(Some(importer()), || crate::share_fluent::admit(
                &mut receiver,
                &r.manifest.share_id,
                g
            ))
            .is_err()
        );
        assert_eq!(receiver.latest_tx_id().unwrap(), before);
        assert!(receiver.lookup("urn:carried-event").unwrap().is_none());
        super::governed_tests::authorize(&mut receiver, &p);
        let before = receiver.latest_tx_id().unwrap();
        assert!(crate::share_fluent::admit(&mut receiver, &r.manifest.share_id, g).is_err());
        assert_eq!(receiver.latest_tx_id().unwrap(), before);
        let tx = with_identity(Some(importer()), || {
            crate::share_fluent::admit(&mut receiver, &r.manifest.share_id, g)
        })
        .unwrap();
        assert_eq!(
            receiver.transaction_auth(tx).unwrap().unwrap().principal,
            "urn:carrier-importer"
        );
        assert_eq!(
            Proof::retained(&receiver, g, "urn:carried-event")
                .unwrap()
                .body,
            p.body
        );
        assert_eq!(
            with_identity(Some(importer()), || crate::share_fluent::admit(
                &mut receiver,
                &r.manifest.share_id,
                g
            ))
            .unwrap(),
            tx
        );
        assert_eq!(receiver.latest_tx_id().unwrap(), tx);
        assert_eq!(
            crate::share_fluent::imported_proofs(&receiver, &r.manifest.share_id)
                .unwrap()
                .unwrap(),
            pending
        );
    }
}

#[test]
fn forged_origin_and_corrupt_retained_manifest_refuse_without_event_allocation() {
    let p = proof(crate::schema::ROOT_GRAPH_IRI);
    let mut forged = p.clone();
    forged.body.observation.order.writer = "urn:forged-source-actor".into();
    let r = request(&[forged]);
    let (mut receiver, g) = super::tests::loaded_shapes(crate::schema::ROOT_GRAPH_IRI);
    receiver.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
    super::governed_tests::authorize(&mut receiver, &p);
    import_share(&mut receiver, &r, "2026-11-08T00:00:00Z", None).unwrap();
    let before = receiver.latest_tx_id().unwrap();
    assert!(
        with_identity(Some(importer()), || crate::share_fluent::admit(
            &mut receiver,
            &r.manifest.share_id,
            g
        ))
        .is_err()
    );
    assert_eq!(receiver.latest_tx_id().unwrap(), before);
    assert!(receiver.lookup("urn:carried-event").unwrap().is_none());
    receiver
        .conn
        .execute(
            "UPDATE share_fluent_carriers SET manifest='{}' WHERE share_id=?1",
            [&r.manifest.share_id],
        )
        .unwrap();
    assert!(crate::share_fluent::imported_proofs(&receiver, &r.manifest.share_id).is_err());
    assert!(
        with_identity(Some(importer()), || crate::share_fluent::admit(
            &mut receiver,
            &r.manifest.share_id,
            g
        ))
        .is_err()
    );
    assert_eq!(receiver.latest_tx_id().unwrap(), before);
}
