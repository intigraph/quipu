//! Pending pendant bytes bind evidence without borrowing source authority.
use super::*;
use crate::Error;
use crate::pack::{PackOptions, pack_to_bytes, pack_to_bytes_with_carriers, pack_with_carriers};
use crate::share::{ShareDestination, ShareManifest};
use crate::transaction_auth::with_identity;

const AT: &str = "2026-11-08T02:00:00Z";

#[path = "source_materialization_tests.rs"]
mod materialization_tests;

fn options() -> PackOptions {
    PackOptions {
        destination: ShareDestination::Internal,
        ..Default::default()
    }
}

fn manifest(proofs: Vec<String>) -> ShareManifest {
    crate::share::share_payload(
        &Store::open_in_memory().unwrap(),
        &crate::share::ShareOptions {
            no_shapes: true,
            queries: Some(Vec::new()),
            destination: ShareDestination::Internal,
            fluent_proofs: proofs,
            ..Default::default()
        },
        crate::share::SHARE_PAYLOAD_MAX_BYTES,
    )
    .unwrap()
    .manifest
}

fn bytes(manifests: &[ShareManifest]) -> (crate::pack::Manifest, Vec<u8>) {
    pack_to_bytes_with_carriers(
        &Store::open_in_memory().unwrap(),
        crate::schema::ROOT_GRAPH_IRI,
        &options(),
        AT,
        manifests,
    )
    .unwrap()
}

#[test]
fn explicit_pendant_empty_extension_keeps_legacy_hash_and_selection_is_opt_in() {
    let relay = Store::open_in_memory().unwrap();
    let ordinary = pack_to_bytes(&relay, crate::schema::ROOT_GRAPH_IRI, &options(), AT)
        .unwrap()
        .0;
    let explicit =
        pack_to_bytes_with_carriers(&relay, crate::schema::ROOT_GRAPH_IRI, &options(), AT, &[])
            .unwrap()
            .0;
    assert_eq!(ordinary.content_hash, explicit.content_hash);
    let pending = manifest(vec!["{}".into()]);
    crate::share_fluent::retain(&relay, &pending).unwrap();
    let unselected = pack_to_bytes(&relay, crate::schema::ROOT_GRAPH_IRI, &options(), AT)
        .unwrap()
        .0;
    assert_eq!(ordinary.content_hash, unselected.content_hash);
    assert_ne!(ordinary.content_hash, bytes(&[pending]).0.content_hash);
}

#[test]
fn explicit_pendant_file_bytes_retain_proofs_but_require_fresh_native_authority() {
    for graph in [crate::schema::ROOT_GRAPH_IRI, "urn:graph-a", "urn:graph-b"] {
        let proof = super::carrier_tests::proof(graph);
        let request = super::carrier_tests::request(std::slice::from_ref(&proof));
        let (expected, payload) = bytes(std::slice::from_ref(&request.manifest));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pending.pendant.db");
        std::fs::write(&path, payload).unwrap();
        assert!(crate::pack::verify(path.to_str().unwrap()).unwrap().2);
        let disk = dir.path().join("disk.pendant.db");
        let built = pack_with_carriers(
            &Store::open_in_memory().unwrap(),
            crate::schema::ROOT_GRAPH_IRI,
            disk.to_str().unwrap(),
            &options(),
            AT,
            std::slice::from_ref(&request.manifest),
        )
        .unwrap();
        assert_eq!(expected.content_hash, built.content_hash);
        assert!(crate::pack::verify(disk.to_str().unwrap()).unwrap().2);
        let mut receiver = Store::open(path.to_str().unwrap()).unwrap();
        receiver.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
        assert_eq!(
            crate::share_fluent::imported_proofs(&receiver, &request.manifest.share_id).unwrap(),
            Some(request.manifest.fluent_proofs.clone())
        );
        assert!(receiver.lookup("urn:carried-event").unwrap().is_none());
        let receipts: i64 = receiver
            .conn
            .query_row("SELECT count(*) FROM fluent_origin_receipts", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(receipts, 0);
        receiver
            .load_shapes(
                "quipu-fluents-v1",
                include_str!("../../shapes/fluents-v1.ttl"),
                AT,
            )
            .unwrap();
        receiver.load_shapes("declarations", "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix qp: <https://w3id.org/intigraph/quipu/ns#> . <urn:shape> sh:path <urn:count>; sh:targetNode <urn:subject>; qp:currentValuePolicy \"sum\" .", AT).unwrap();
        let g = if graph == crate::schema::ROOT_GRAPH_IRI {
            0
        } else {
            receiver.graph_create(graph).unwrap()
        };
        assert!(
            with_identity(Some(super::carrier_tests::importer()), || {
                crate::share_fluent::admit(&mut receiver, &request.manifest.share_id, g)
            })
            .is_err(),
            "a valid pendant and forged actor grant no source authority"
        );
        super::governed_tests::authorize(&mut receiver, &proof);
        let tx = with_identity(Some(super::carrier_tests::importer()), || {
            crate::share_fluent::admit(&mut receiver, &request.manifest.share_id, g)
        })
        .unwrap();
        assert_eq!(
            Proof::retained(&receiver, g, "urn:carried-event")
                .unwrap()
                .body,
            proof.body
        );
        assert_eq!(
            with_identity(Some(super::carrier_tests::importer()), || {
                crate::share_fluent::admit(&mut receiver, &request.manifest.share_id, g)
            })
            .unwrap(),
            tx
        );
        // A generic materializing loader must refuse before creating a target,
        // rather than silently dropping the evidence it cannot materialize.
        let destination = dir.path().join("ordinary-load.db");
        assert!(
            crate::pack::unpack_verified(
                disk.to_str().unwrap(),
                destination.to_str().unwrap(),
                &Default::default(),
                AT
            )
            .is_err()
        );
        assert!(!destination.exists());
    }
}

#[test]
fn explicit_pendant_hash_binds_complete_carriers_and_normalizes_only_table_order() {
    let a = manifest(vec![" {\"a\":1} ".into()]);
    let b = manifest(vec!["{\"b\":2}".into()]);
    assert_eq!(
        bytes(&[a.clone(), b.clone()]).0.content_hash,
        bytes(&[b.clone(), a.clone()]).0.content_hash
    );
    assert_eq!(
        bytes(std::slice::from_ref(&a)).0.content_hash,
        bytes(&[a.clone(), a.clone()]).0.content_hash
    );
    for action in ["strip", "inject", "change", "malformed", "oversize"] {
        let (_, payload) = bytes(std::slice::from_ref(&a));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tampered.db");
        std::fs::write(&path, payload).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        match action {
            "strip" => {
                conn.execute("DELETE FROM share_fluent_carriers", [])
                    .unwrap();
            }
            "inject" => {
                conn.execute(
                    "INSERT INTO share_fluent_carriers VALUES(?1,?2)",
                    rusqlite::params![
                        b.share_id,
                        String::from_utf8(crate::share::manifest_bytes(&b, true).unwrap()).unwrap()
                    ],
                )
                .unwrap();
            }
            "change" => {
                conn.execute(
                    "UPDATE share_fluent_carriers SET manifest=manifest||' '",
                    [],
                )
                .unwrap();
            }
            "malformed" => {
                conn.execute("UPDATE share_fluent_carriers SET manifest='{'", [])
                    .unwrap();
            }
            "oversize" => {
                conn.execute(
                    "UPDATE share_fluent_carriers SET manifest=zeroblob(33554433)",
                    [],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        drop(conn);
        assert!(
            !matches!(
                crate::pack::verify(path.to_str().unwrap()),
                Ok((_, _, true))
            ),
            "{action} must not verify"
        );
    }
}

#[test]
fn explicit_pendant_refuses_bad_identity_count_and_empty_evidence() {
    let relay = Store::open_in_memory().unwrap();
    let a = manifest(vec!["{}".into()]);
    let mut invalid = a.clone();
    invalid.fluent_proofs.push("{}".into());
    for pending in [
        vec![invalid],
        vec![a.clone(); 65],
        vec![manifest(Vec::new())],
    ] {
        assert!(
            pack_to_bytes_with_carriers(
                &relay,
                crate::schema::ROOT_GRAPH_IRI,
                &options(),
                AT,
                &pending
            )
            .is_err()
        );
    }
}

#[test]
fn explicit_pendant_conflicting_bundle_attestations_never_collapse_by_share_id() {
    let a = manifest(vec!["{}".into()]);
    let mut b = a.clone();
    let binding = crate::session_attestation::SessionBinding::new(
        "forged-source-actor",
        "fixture-session",
        super::tests::signer(9).public_key_hex(),
        "fixture-introducer",
        1,
        100,
    )
    .unwrap();
    b.attestation = Some(crate::share::ShareAttestation {
        envelope: crate::session_attestation::AttestationEnvelope {
            version: "quipu-share-v1".into(),
            key_id: binding.key_id.clone(),
            session: binding.session.clone(),
            introducer: binding.introducer.clone(),
            issued_at_epoch: 1,
            nonce: "00".repeat(16),
            signature: "forged".into(),
            audience: None,
        },
        binding,
    });
    // Bundle attestations are excluded from share_id, but not from the pendant
    // hash. They remain opaque claims and cannot silently replace each other.
    crate::share_fluent::check(&b).unwrap();
    assert_eq!(a.share_id, b.share_id);
    assert_ne!(
        bytes(std::slice::from_ref(&a)).0.content_hash,
        bytes(std::slice::from_ref(&b)).0.content_hash
    );
    assert!(
        pack_to_bytes_with_carriers(
            &Store::open_in_memory().unwrap(),
            crate::schema::ROOT_GRAPH_IRI,
            &options(),
            AT,
            &[a, b]
        )
        .is_err()
    );
}

#[test]
fn explicit_pendant_inspects_raw_escaped_and_shadowed_evidence_without_rewriting_it() {
    let mut relay = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut relay);
    for proof in [
        r#"{"value":"FIXTURE_PRIVATE_TOKEN"}"#,
        r#"{"value":"\u0046IXTURE_PRIVATE_TOKEN"}"#,
        r#"{"value":"\u0046IXTURE_PRIVATE_TOKEN","value":"public"}"#,
        r#"{"\u0046IXTURE_PRIVATE_TOKEN":"public"}"#,
    ] {
        let pending = manifest(vec![proof.into()]);
        let outward = PackOptions {
            destination: ShareDestination::Outward,
            ..Default::default()
        };
        assert!(matches!(
            pack_to_bytes_with_carriers(
                &relay,
                crate::schema::ROOT_GRAPH_IRI,
                &outward,
                AT,
                std::slice::from_ref(&pending)
            ),
            Err(Error::PolicyDenied(_))
        ));
        let (_, payload) = pack_to_bytes_with_carriers(
            &relay,
            crate::schema::ROOT_GRAPH_IRI,
            &options(),
            AT,
            std::slice::from_ref(&pending),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("internal.db");
        std::fs::write(&path, payload).unwrap();
        let retained = Store::open(path.to_str().unwrap()).unwrap();
        assert_eq!(
            crate::share_fluent::imported_proofs(&retained, &pending.share_id).unwrap(),
            Some(vec![proof.into()])
        );
    }
}
