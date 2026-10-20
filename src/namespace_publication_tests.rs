use super::*;
use crate::entity_namespaces::{DestinationBinding, Namespace};
use crate::share::{ShareOptions, share_payload};
use serde_json::json;

fn policy(store: &mut Store, target: &str) {
    let mut cfg = crate::QuipuConfig::default();
    cfg.namespaces.push(Namespace {
        prefix: "team".into(),
        base_iri: "https://team.example/id/".into(),
        visibility: "private".into(),
        audience: "org:example".into(),
        resolver_url: Some("https://team.example/entity?iri={iri}".into()),
        ..Namespace::default()
    });
    cfg.namespace_destinations.push(DestinationBinding {
        store_id: target.into(),
        audience: "org:example".into(),
    });
    entity_namespaces::configure(store, &cfg).unwrap();
    entity_namespaces::apply(store).unwrap();
}
fn destination(target: &str) -> ShareDestination {
    ShareDestination::Audience(DestinationBinding {
        store_id: target.into(),
        audience: "org:example".into(),
    })
}

#[test]
fn namespace_policy_checks_every_file_and_escaped_nested_manifest() {
    let mut store = Store::open_in_memory().unwrap();
    policy(&mut store, "receiver");
    for (name, text) in [
        ("export.nt", "<https://team.example/id/s> <urn:p> <urn:o> ."),
        ("export.nt", "<urn:s> <https://team.example/id/p> <urn:o> ."),
        ("export.nt", "<urn:s> <urn:p> <https://team.example/id/o> ."),
        (
            "export.nt",
            "<urn:s> <urn:p> \"x\"^^<https://team.example/id/type> .",
        ),
        (
            "queries.ttl",
            "SELECT * FROM <https://team.example/id/graph> WHERE {?s ?p ?o}",
        ),
        (
            "manifest.json",
            r#"{"a":{"b":[{"https:\/\/team.example/id/key":"value"}]}}"#,
        ),
        (
            "manifest.json",
            r#"{"nested":"https://te\u0061m.example/id/x"}"#,
        ),
        (
            "shapes.ttl",
            "<urn:s> <urn:p> <https%3A%2F%2Fteam.example%2Fid%2Fx> .",
        ),
    ] {
        let files = BTreeMap::from([(name.into(), text.into())]);
        assert!(
            crate::share_scrub::enforce_destination(
                &store,
                &files,
                ShareDestination::Outward,
                "test"
            )
            .is_err(),
            "{text}"
        );
        assert!(
            enforce(&store, &files, &ShareDestination::Internal).is_err(),
            "legacy internal is not audience proof"
        );
        assert!(enforce(&store, &files, &destination("forged")).is_err());
        enforce(&store, &files, &destination("receiver")).unwrap();
    }
    enforce(
        &store,
        &BTreeMap::from([("manifest.json".into(), "public control".into())]),
        &ShareDestination::Outward,
    )
    .unwrap();
}

#[test]
fn actual_org_pendant_import_resolves_in_second_store_public_and_forged_exports_refuse() {
    let mut source = Store::open_in_memory().unwrap();
    let mut receiver = Store::open_in_memory().unwrap();
    let receiver_id = receiver.store_id().unwrap();
    assert_ne!(receiver_id, source.store_id().unwrap());
    policy(&mut source, &receiver_id);
    policy(&mut receiver, &receiver_id);
    crate::share_scrub::seed_test_catalogue(&mut source);
    crate::share_scrub::seed_test_catalogue(&mut receiver);
    let minted = crate::entity_reference::tool_entity_ref(
        &mut source,
        &json!({
            "action":"mint","namespace":"team","type":"urn:example:Host","label":"Team hardware"
        }),
    )
    .unwrap();
    let options = ShareOptions {
        no_shapes: true,
        destination: destination(&receiver_id),
        ..Default::default()
    };
    let payload = share_payload(&source, &options, 1024 * 1024).unwrap();
    assert_eq!(
        payload.manifest.destination,
        Some(destination(&receiver_id))
    );
    let public = ShareOptions {
        destination: ShareDestination::Outward,
        ..options.clone()
    };
    assert!(share_payload(&source, &public, 1024 * 1024).is_err());
    let legacy = ShareOptions {
        destination: ShareDestination::Internal,
        ..options.clone()
    };
    assert!(share_payload(&source, &legacy, 1024 * 1024).is_err());
    let forged = ShareOptions {
        destination: destination("different-store"),
        ..options
    };
    assert!(share_payload(&source, &forged, 1024 * 1024).is_err());
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in &payload.files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let mut incoming =
        crate::share_transport::read_reference(dir.path().to_str().unwrap()).unwrap();
    incoming.destination = destination(&receiver_id);
    let result =
        crate::share_import::import_share(&mut receiver, &incoming, "2026-10-20T00:00:00Z", None)
            .unwrap();
    assert!(result.triples.accepted + result.triples.quarantined > 0);
    let response = crate::entity_reference::tool_entity_ref(
        &mut receiver,
        &json!({"action":"resolve","iri":minted["iri"]}),
    )
    .unwrap();
    assert_eq!(response["known"], true);
    assert_eq!(response["audience"], "org:example");
    assert!(
        response["resolver_url"]
            .as_str()
            .unwrap()
            .starts_with("https://team.example/")
    );
    let id = receiver
        .lookup(minted["iri"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert!(
        !receiver
            .current_facts_for_entities_in_graphs(
                &[id],
                &[receiver.lookup(&result.staging_graph).unwrap().unwrap()]
            )
            .unwrap()
            .is_empty()
    );
    let mut stranger = Store::open_in_memory().unwrap();
    policy(&mut stranger, &receiver_id);
    assert!(
        crate::share_import::import_share(&mut stranger, &incoming, "2026-10-20T00:00:00Z", None)
            .is_err()
    );
    assert!(
        stranger
            .lookup(minted["iri"].as_str().unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn public_share_positive_and_destination_provenance_cannot_be_downgraded() {
    let mut store = Store::open_in_memory().unwrap();
    policy(&mut store, "receiver");
    crate::share_scrub::seed_test_catalogue(&mut store);
    crate::rdf::ingest_rdf(
        &mut store,
        b"<https://public.example/id/a> <urn:p> \"public\" .".as_slice(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-10-20T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    share_payload(
        &store,
        &ShareOptions {
            no_shapes: true,
            ..Default::default()
        },
        1024 * 1024,
    )
    .unwrap();
    let d = destination("receiver");
    assert_eq!(
        serde_json::from_value::<ShareDestination>(serde_json::to_value(&d).unwrap()).unwrap(),
        d
    );
    assert_eq!(
        serde_json::to_string(&ShareDestination::Internal).unwrap(),
        "\"internal\""
    );
}

#[test]
fn audience_binary_pack_requires_actual_receiver_and_preserves_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.db");
    let mut receiver = Store::open(target.to_str().unwrap()).unwrap();
    let receiver_id = receiver.store_id().unwrap();
    policy(&mut receiver, &receiver_id);
    crate::share_scrub::seed_test_catalogue(&mut receiver);
    entity_namespaces::apply(&mut receiver).unwrap();
    let mut cfg = crate::QuipuConfig {
        namespaces: entity_namespaces::list(&receiver).unwrap(),
        ..Default::default()
    };
    cfg.namespace_destinations.push(DestinationBinding {
        store_id: receiver_id.clone(),
        audience: "org:example".into(),
    });
    drop(receiver);
    let mut source = Store::open_in_memory().unwrap();
    policy(&mut source, &receiver_id);
    crate::share_scrub::seed_test_catalogue(&mut source);
    let graph = source.graph_create("urn:example:team-graph").unwrap();
    crate::rdf::ingest_rdf_to_graph(
        &mut source,
        b"<https://team.example/id/object> <urn:p> \"positive\" .".as_slice(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-10-20T00:00:00Z",
        None,
        None,
        graph,
    )
    .unwrap();
    let out = dir.path().join("team.pendant.db");
    let options = crate::pack::PackOptions {
        destination: destination(&receiver_id),
        ..Default::default()
    };
    crate::pack::pack(
        &source,
        "urn:example:team-graph",
        out.to_str().unwrap(),
        &options,
        "2026-10-20T00:00:00Z",
    )
    .unwrap();
    assert_eq!(
        crate::pack::read_manifest(out.to_str().unwrap())
            .unwrap()
            .destination,
        Some(destination(&receiver_id))
    );
    assert!(
        crate::pack::unpack_verified(
            out.to_str().unwrap(),
            target.to_str().unwrap(),
            &Default::default(),
            "2026-10-20T00:00:00Z"
        )
        .is_err()
    );
    let loaded = crate::pack::unpack_verified(
        out.to_str().unwrap(),
        target.to_str().unwrap(),
        &crate::pack::LoadOptions {
            namespace_policy: Some(&cfg),
            destination: destination(&receiver_id),
            ..Default::default()
        },
        "2026-10-20T00:00:00Z",
    )
    .unwrap();
    assert_eq!(loaded.outcome, "loaded");
    assert!(loaded.facts > 0);
    let forbidden = dir.path().join("public.pendant.db");
    assert!(
        crate::pack::pack(
            &source,
            "urn:example:team-graph",
            forbidden.to_str().unwrap(),
            &Default::default(),
            "2026-10-20T00:00:00Z"
        )
        .is_err()
    );
    assert!(!forbidden.exists());
}

#[test]
fn escaped_unicode_namespace_and_personal_grants_are_not_public_bypasses() {
    let mut store = Store::open_in_memory().unwrap();
    let mut cfg = crate::QuipuConfig::default();
    cfg.namespaces.push(Namespace {
        prefix: "mine".into(),
        base_iri: "https://private.example/café/".into(),
        visibility: "private".into(),
        audience: "personal:owner".into(),
        ..Namespace::default()
    });
    cfg.namespace_destinations.push(DestinationBinding {
        store_id: "owner-second".into(),
        audience: "personal:owner".into(),
    });
    cfg.namespace_destinations.push(DestinationBinding {
        store_id: "public".into(),
        audience: "public".into(),
    });
    entity_namespaces::configure(&mut store, &cfg).unwrap();
    let files = BTreeMap::from([(
        "manifest.json".into(),
        "https%3A%2F%2Fprivate.example%2Fcaf%C3%A9%2Fx".into(),
    )]);
    assert!(enforce(&store, &files, &ShareDestination::Outward).is_err());
    assert!(
        enforce(
            &store,
            &files,
            &ShareDestination::Audience(DestinationBinding {
                store_id: "public".into(),
                audience: "public".into()
            })
        )
        .is_err()
    );
    enforce(
        &store,
        &files,
        &ShareDestination::Audience(DestinationBinding {
            store_id: "owner-second".into(),
            audience: "personal:owner".into(),
        }),
    )
    .unwrap();
}

#[test]
fn json_surrogate_pairs_cannot_hide_a_private_namespace() {
    let mut store = Store::open_in_memory().unwrap();
    let cfg = crate::QuipuConfig {
        namespaces: vec![Namespace {
            prefix: "team".into(),
            base_iri: "https://team.example/💡/".into(),
            visibility: "private".into(),
            audience: "org:example".into(),
            ..Default::default()
        }],
        namespace_destinations: vec![DestinationBinding {
            store_id: "receiver".into(),
            audience: "org:example".into(),
        }],
        ..Default::default()
    };
    entity_namespaces::configure(&mut store, &cfg).unwrap();
    for payload in [
        r#"{"nested":{"iri":"https://team.example/\ud83d\udca1/x"}}"#,
        r#"{"nested":"{\"iri\":\"https://team.example/\\ud83d\\udca1/x\"}"}"#,
    ] {
        let files = BTreeMap::from([("manifest.json".into(), payload.into())]);
        assert!(enforce(&store, &files, &ShareDestination::Outward).is_err());
        enforce(&store, &files, &destination("receiver")).unwrap();
    }
}

#[test]
fn org_transfer_retains_identifier_catalogue_and_only_exempts_registered_iris() {
    let mut store = Store::open_in_memory().unwrap();
    policy(&mut store, "receiver");
    crate::share_scrub::seed_test_catalogue(&mut store);
    let allowed = BTreeMap::from([(
        "export.nt".into(),
        "<https://team.example/id/FIXTURE_PRIVATE_TOKEN> <urn:p> <urn:o> .".into(),
    )]);
    crate::share_scrub::enforce_destination(&store, &allowed, destination("receiver"), "test")
        .unwrap();
    for (name, text) in [
        (
            "export.nt",
            "<https://team.example/id/x> <urn:p> \"FIXTURE_PRIVATE_TOKEN\" .",
        ),
        (
            "manifest.json",
            r#"{"nested":{"secret":"FIXTURE_PRIVATE_TOKEN"}}"#,
        ),
    ] {
        let files = BTreeMap::from([(name.into(), text.into())]);
        assert!(
            crate::share_scrub::enforce_destination(
                &store,
                &files,
                destination("receiver"),
                "test"
            )
            .is_err()
        );
    }
}
