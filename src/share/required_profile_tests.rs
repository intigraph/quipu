use super::*;
use crate::share_import::{ShareImportRequest, import_share};
use crate::share_scrub::ShareDestination;

const TS: &str = "2026-11-14T00:00:00Z";
const PROFILE: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:profile:Thing> a sh:NodeShape ; sh:targetClass <urn:Thing> .\n";
const REF: &str = "seeds-v0.22.1@30e17964";

fn request(declared: bool) -> ShareImportRequest {
    let mut producer = Store::open_in_memory().unwrap();
    producer.load_shapes(REF, PROFILE, TS).unwrap();
    crate::rdf::ingest_rdf(
        &mut producer,
        &b"<urn:one> a <urn:Thing> ."[..],
        oxrdfio::RdfFormat::Turtle,
        None,
        TS,
        None,
        None,
    )
    .unwrap();
    let payload = share_payload(
        &producer,
        &ShareOptions {
            required_profile: declared.then(|| RequiredProfile {
                reference: REF.into(),
                digest: sha256(PROFILE.as_bytes()),
            }),
            destination: ShareDestination::Internal,
            ..Default::default()
        },
        SHARE_PAYLOAD_MAX_BYTES,
    )
    .unwrap();
    ShareImportRequest {
        manifest: payload.manifest,
        export_ntriples: payload.files["export.nt"].clone(),
        shapes_turtle: payload.files["shapes.ttl"].clone(),
        queries_turtle: None,
        alignments_turtle: None,
        competency_toml: None,
        commands_turtle: None,
        query_namespace: None,
        replace_queries: false,
        source: "fixture".into(),
        actor: None,
        accept_exact: false,
        destination: ShareDestination::Internal,
        #[cfg(not(target_arch = "wasm32"))]
        attestation: None,
    }
}

#[test]
fn required_profile_absent_or_wrong_ref_or_digest_refuses_before_staging() {
    let request = request(true);
    for local in [
        None,
        Some(("other-release", PROFILE)),
        Some((REF, "# different bytes\n")),
    ] {
        let mut recipient = Store::open_in_memory().unwrap();
        if let Some((name, turtle)) = local {
            recipient.load_shapes(name, turtle, TS).unwrap();
        }
        let before = recipient.latest_tx_id().unwrap();
        let before_events: i64 = recipient
            .conn
            .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
            .unwrap();
        let error = import_share(&mut recipient, &request, TS, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("quipu shapes load"), "{error}");
        assert!(
            error.contains(REF) && error.contains(&sha256(PROFILE.as_bytes())),
            "{error}"
        );
        assert_eq!(recipient.latest_tx_id().unwrap(), before);
        for table in ["import_reviews", "events"] {
            let exists: i64 = recipient
                .conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(
                exists,
                if table == "events" { before_events } else { 0 },
                "refusal wrote {table}"
            );
        }
    }
}

#[cfg(feature = "shacl")]
#[test]
fn required_profile_present_admits_but_undeclared_still_quarantines() {
    let mut recipient = Store::open_in_memory().unwrap();
    recipient.load_shapes(REF, PROFILE, TS).unwrap();
    let result = import_share(&mut recipient, &request(true), TS, None).unwrap();
    assert!(result.promotion.eligible, "{:?}", result.promotion.blockers);
    let mut empty = Store::open_in_memory().unwrap();
    let legacy = request(false);
    assert!(
        !serde_json::to_value(&legacy.manifest)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("required_profile")
    );
    let result = import_share(&mut empty, &legacy, TS, None).unwrap();
    assert!(
        result
            .promotion
            .blockers
            .contains(&"off_vocabulary".to_string())
    );
    assert!(
        empty.list_shapes().unwrap().is_empty(),
        "incoming profile must not install"
    );
}

#[test]
fn required_profile_is_sealed_and_malformed_requirements_fail_closed() {
    let mut req = request(true);
    req.manifest.required_profile.as_mut().unwrap().reference = "other-release".into();
    assert!(crate::share_import::verify_share(&req).is_err());
    for reference in ["", "-option", "ref with spaces", "ref;command"] {
        assert!(
            RequiredProfile {
                reference: reference.into(),
                digest: sha256(PROFILE.as_bytes())
            }
            .validate()
            .is_err()
        );
    }
    for digest in [
        "",
        "sha256:short",
        "b1b3",
        "sha256:FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF",
    ] {
        assert!(
            RequiredProfile {
                reference: REF.into(),
                digest: digest.into()
            }
            .validate()
            .is_err()
        );
    }
}
