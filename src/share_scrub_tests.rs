use super::*;

#[test]
fn empty_catalogue_cannot_verify_outward_payload() {
    let store = Store::open_in_memory().unwrap();
    let files = BTreeMap::from([("export.nt".into(), "clean".into())]);
    assert!(matches!(
        scrub_outward_payload(&store, &files, "test"),
        Err(Error::CannotVerify(_))
    ));
}

#[test]
fn named_catalogue_rejects_a_real_violation_and_accepts_clean_payload() {
    let mut store = Store::open_in_memory().unwrap();
    seed_test_catalogue(&mut store);
    let clean = BTreeMap::from([("export.nt".into(), "clean".into())]);
    scrub_outward_payload(&store, &clean, "test").unwrap();
    let bad = BTreeMap::from([("export.nt".into(), "FIXTURE_PRIVATE_TOKEN".into())]);
    assert!(matches!(
        scrub_outward_payload(&store, &bad, "test"),
        Err(Error::PolicyDenied(_))
    ));
}

#[test]
fn split_catalogue_statements_do_not_manufacture_a_rule() {
    let mut store = Store::open_in_memory().unwrap();
    for (name, ttl) in [
        (
            "urn:test:one",
            "<urn:rule> a <http://aegis.gastown.local/ontology/InternalIdentifierPattern> ; <http://www.w3.org/2000/01/rdf-schema#label> \"split\" .",
        ),
        (
            "urn:test:two",
            "<urn:rule> <http://aegis.gastown.local/ontology/regex> \"secret\" ; <http://aegis.gastown.local/ontology/enforcementTier> \"block\" .",
        ),
    ] {
        let graph = store.overlay_create(name, 0).unwrap();
        crate::rdf::ingest_rdf_to_graph(
            &mut store,
            ttl.as_bytes(),
            oxrdfio::RdfFormat::Turtle,
            None,
            "2026-08-01T00:00:00Z",
            None,
            Some("test"),
            graph,
        )
        .unwrap();
    }
    assert!(matches!(
        outward_scrub_patterns(&store),
        Err(Error::CannotVerify(_))
    ));
}

fn carried_payload(
    proof: &str,
    destination: ShareDestination,
) -> Result<crate::share::SharePayload> {
    let mut store = Store::open_in_memory().unwrap();
    seed_test_catalogue(&mut store);
    crate::share::share_payload(
        &store,
        &crate::share::ShareOptions {
            no_shapes: true,
            queries: Some(Vec::new()),
            destination,
            fluent_proofs: vec![proof.into()],
            ..Default::default()
        },
        crate::share::SHARE_PAYLOAD_MAX_BYTES,
    )
}

#[test]
fn fluent_carrier_outward_refuses_private_raw_value() {
    assert!(matches!(
        carried_payload(
            r#"{"event":{"value":"FIXTURE_PRIVATE_TOKEN"}}"#,
            ShareDestination::Outward
        ),
        Err(Error::PolicyDenied(_))
    ));
}

#[test]
fn fluent_carrier_outward_refuses_private_escaped_value() {
    assert!(matches!(
        carried_payload(
            r#"{"event":{"value":"\u0046IXTURE_PRIVATE_TOKEN"}}"#,
            ShareDestination::Outward
        ),
        Err(Error::PolicyDenied(_))
    ));
}

#[test]
fn fluent_carrier_clean_bytes_and_internal_destination_remain_explicit() {
    let clean = " {\"event\":{\"value\":\"public\\u0020value\"}} ";
    let payload = carried_payload(clean, ShareDestination::Outward).unwrap();
    assert_eq!(payload.manifest.fluent_proofs, [clean]);
    let internal = carried_payload(
        r#"{"event":{"value":"FIXTURE_PRIVATE_TOKEN"}}"#,
        ShareDestination::Internal,
    )
    .unwrap();
    assert_eq!(internal.manifest.fluent_proofs.len(), 1);
}

#[test]
fn fluent_carrier_outward_refuses_escaped_keys_and_shadowed_fields() {
    for proof in [
        r#"{"\u0046IXTURE_PRIVATE_TOKEN":"public"}"#,
        r#"{"value":"\u0046IXTURE_PRIVATE_TOKEN","value":"public"}"#,
    ] {
        assert!(matches!(
            carried_payload(proof, ShareDestination::Outward),
            Err(Error::PolicyDenied(_))
        ));
    }
}

#[test]
fn fluent_carrier_outward_cannot_verify_malformed_json() {
    assert!(matches!(
        carried_payload("{", ShareDestination::Outward),
        Err(Error::CannotVerify(_))
    ));
}

#[test]
fn fluent_carrier_inspection_never_hides_a_colliding_artifact() {
    let files = BTreeMap::from([
        ("manifest.json".into(), r#"{"fluent_proofs":["{}"]}"#.into()),
        ("fluent proof 0 wire".into(), "FIXTURE_PRIVATE_TOKEN".into()),
    ]);
    let inspected = crate::share_fluent::inspection_files(&files)
        .unwrap()
        .unwrap();
    let mut store = Store::open_in_memory().unwrap();
    seed_test_catalogue(&mut store);
    assert!(matches!(
        scrub_outward_payload(&store, &inspected, "test"),
        Err(Error::PolicyDenied(_))
    ));
    assert_eq!(files["fluent proof 0 wire"], "FIXTURE_PRIVATE_TOKEN");
}
