use super::*;
use crate::entity_namespaces::{apply, configure};

fn fixture() -> (Store, crate::QuipuConfig, RemovalRequest) {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    let mut config = crate::QuipuConfig::default();
    config.namespaces.push(Namespace {
        prefix: "fixture".into(),
        base_iri: "https://example.test/private/".into(),
        visibility: "private".into(),
        audience: "org:test".into(),
        graphs: vec!["urn:example:graph".into()],
        types: vec!["urn:example:Host".into()],
        ..Namespace::default()
    });
    configure(&mut store, &config).unwrap();
    apply(&mut store).unwrap();
    configure(&mut store, &crate::QuipuConfig::default()).unwrap();
    let request = RemovalRequest {
        expected_registry_hash: registry_hash(&store).unwrap(),
        namespace: config.namespaces[0].clone(),
        max_reference_facts: default_inventory_limit(),
    };
    (store, config, request)
}

#[test]
fn unused_private_policy_roundtrip_preserves_history_and_exact_effective_registry() {
    let (mut store, config, request) = fixture();
    let transactions = store.latest_tx_id().unwrap();
    remove(&mut store, &request).unwrap();
    assert!(list(&store).unwrap().is_empty());
    assert!(store.latest_tx_id().unwrap() > transactions);
    let history: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM facts WHERE op=1 AND valid_to IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(history > 0);
    assert!(remove(&mut store, &request).is_err());
    configure(&mut store, &config).unwrap();
    let restore = RemovalRequest {
        expected_registry_hash: registry_hash(&store).unwrap(),
        ..request
    };
    apply_if(&mut store, &restore).unwrap();
    assert_eq!(list(&store).unwrap(), config.namespaces);
}

#[test]
fn stale_hash_selectors_and_config_presence_refuse_without_retraction() {
    let (mut store, config, mut request) = fixture();
    request.namespace.types.clear();
    assert!(remove(&mut store, &request).is_err());
    request.namespace = config.namespaces[0].clone();
    configure(&mut store, &config).unwrap();
    request.expected_registry_hash = registry_hash(&store).unwrap();
    assert!(remove(&mut store, &request).is_err());
    configure(&mut store, &crate::QuipuConfig::default()).unwrap();
    request.expected_registry_hash = registry_hash(&store).unwrap();
    store
        .transact(&[], &crate::time::now_iso(), None, Some("concurrent"))
        .unwrap();
    assert!(matches!(
        remove(&mut store, &request),
        Err(Error::ConditionalWriteConflict)
    ));
    assert_eq!(list(&store).unwrap(), config.namespaces);
}

#[test]
fn even_dictionary_only_allocations_refuse_and_keep_private_policy() {
    let (mut store, config, mut request) = fixture();
    store
        .intern("https://example.test/private/previously-allocated")
        .unwrap();
    request.expected_registry_hash = registry_hash(&store).unwrap();
    assert!(remove(&mut store, &request).is_err());
    assert_eq!(list(&store).unwrap(), config.namespaces);
}

#[test]
fn local_grants_and_unknown_grant_inventory_refuse() {
    let (mut store, config, request) = fixture();
    store
        .conn
        .execute(
            "INSERT INTO writer_grants VALUES('fixture',1,'unknown')",
            [],
        )
        .unwrap();
    assert!(remove(&mut store, &request).is_err());
    assert_eq!(list(&store).unwrap(), config.namespaces);
    let (mut store, _, request) = fixture();
    store
        .conn
        .execute_batch("DROP TABLE writer_grants")
        .unwrap();
    assert!(remove(&mut store, &request).is_err());
}

#[test]
fn foreign_literal_reference_and_request_principal_refuse() {
    let (mut store, config, mut request) = fixture();
    let graph = store.graph_create("urn:example:foreign").unwrap();
    let e = store.intern("urn:example:reference").unwrap();
    let a = store.intern("urn:example:policy").unwrap();
    store
        .transact_to_graph(
            &[Datum {
                entity: e,
                attribute: a,
                value: Value::Str(config.namespaces[0].base_iri.clone()),
                valid_from: crate::time::now_iso(),
                valid_to: None,
                op: Op::Assert,
            }],
            &crate::time::now_iso(),
            None,
            Some("foreign"),
            graph,
        )
        .unwrap();
    request.expected_registry_hash = registry_hash(&store).unwrap();
    assert!(remove(&mut store, &request).is_err());
    let (mut store, _, request) = fixture();
    let identity = crate::transaction_auth::Identity {
        principal: "urn:example:operator".into(),
        credential_id: None,
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    };
    assert!(
        crate::transaction_auth::with_identity(Some(identity), || remove(&mut store, &request))
            .is_err()
    );
    assert_eq!(list(&store).unwrap().len(), 1);
}

#[test]
fn actual_mint_and_retracted_identity_both_refuse_removal() {
    let (mut store, config, mut request) = fixture();
    let graph = store.graph_create("urn:example:graph").unwrap();
    let minted = crate::entity_reference::tool_entity_ref(&mut store, &serde_json::json!({
        "action":"mint", "namespace":"fixture", "type":"urn:example:Host", "graph":"urn:example:graph"
    })).unwrap();
    request.expected_registry_hash = registry_hash(&store).unwrap();
    assert!(remove(&mut store, &request).is_err());
    let entity = store
        .lookup(minted["iri"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let datums: Vec<_> = store
        .entity_facts_in_graph(entity, graph)
        .unwrap()
        .into_iter()
        .map(|fact| Datum {
            entity: fact.entity,
            attribute: fact.attribute,
            value: fact.value,
            valid_from: crate::time::now_iso(),
            valid_to: None,
            op: Op::Retract,
        })
        .collect();
    store
        .transact_to_graph(
            &datums,
            &crate::time::now_iso(),
            None,
            Some("fixture-retract"),
            graph,
        )
        .unwrap();
    request.expected_registry_hash = registry_hash(&store).unwrap();
    assert!(remove(&mut store, &request).is_err());
    assert_eq!(list(&store).unwrap(), config.namespaces);
}

#[test]
fn bounded_inventory_and_escaped_json_references_refuse() {
    let (mut store, _, mut request) = fixture();
    request.max_reference_facts = 0;
    assert!(remove(&mut store, &request).is_err());
    assert!(references(
        r#"{"namespace":"\u0066ixture"}"#,
        &request.namespace
    ));
}

#[test]
fn apply_remove_returns_the_exact_preexisting_effective_registry() {
    let (_, mut config, mut request) = fixture();
    let mut store = Store::open_in_memory_test_ns().unwrap();
    let retained = Namespace {
        prefix: "retained".into(),
        base_iri: "https://other.example/id/".into(),
        ..Namespace::default()
    };
    let mut initial = crate::QuipuConfig::default();
    initial.namespaces.push(retained.clone());
    configure(&mut store, &initial).unwrap();
    apply(&mut store).unwrap();
    let before = list(&store).unwrap();
    config.namespaces.push(retained.clone());
    configure(&mut store, &config).unwrap();
    apply(&mut store).unwrap();
    let mut remaining = crate::QuipuConfig::default();
    remaining.namespaces.push(retained.clone());
    configure(&mut store, &remaining).unwrap();
    request.expected_registry_hash = registry_hash(&store).unwrap();
    remove(&mut store, &request).unwrap();
    assert_eq!(list(&store).unwrap(), before);
    assert!(
        configure(
            &mut store,
            &crate::QuipuConfig {
                namespaces: vec![Namespace {
                    prefix: "overlap".into(),
                    base_iri: "https://other.example/id/nested/".into(),
                    ..Namespace::default()
                }],
                ..crate::QuipuConfig::default()
            }
        )
        .is_err()
    );
}

#[test]
fn conditional_restore_refuses_a_stale_predecessor() {
    let (mut store, config, request) = fixture();
    remove(&mut store, &request).unwrap();
    configure(&mut store, &config).unwrap();
    assert!(matches!(
        apply_if(&mut store, &request),
        Err(Error::ConditionalWriteConflict)
    ));
}

#[test]
fn two_owner_handles_refuse_a_concurrent_registry_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("owner.db");
    let mut first = Store::open(path.to_str().unwrap()).unwrap();
    let mut config = crate::QuipuConfig::default();
    config.namespaces.push(Namespace {
        prefix: "fixture".into(),
        base_iri: "https://example.test/id/".into(),
        ..Namespace::default()
    });
    configure(&mut first, &config).unwrap();
    apply(&mut first).unwrap();
    configure(&mut first, &crate::QuipuConfig::default()).unwrap();
    let request = RemovalRequest {
        expected_registry_hash: registry_hash(&first).unwrap(),
        namespace: config.namespaces[0].clone(),
        max_reference_facts: 100_000,
    };
    let mut second = Store::open(path.to_str().unwrap()).unwrap();
    config.namespaces.push(Namespace {
        prefix: "other".into(),
        base_iri: "https://other.example/id/".into(),
        ..Namespace::default()
    });
    configure(&mut second, &config).unwrap();
    apply(&mut second).unwrap();
    assert!(matches!(
        remove(&mut first, &request),
        Err(Error::ConditionalWriteConflict)
    ));
    assert_eq!(list(&first).unwrap().len(), 2);
}

#[test]
fn active_mint_grant_refuses_without_changing_enrollment() {
    use crate::writer_scope::{MintScope, WriteGrant, WriteOperation};
    let (mut store, config, mut request) = fixture();
    store.graph_create("urn:example:graph").unwrap();
    let grant = WriteGrant {
        version: 1,
        id: "fixture-grant".into(),
        principal: "urn:example:writer".into(),
        credential_id: "fixture-key".into(),
        instance: store.writer_graph_instance("urn:example:graph").unwrap(),
        operations: std::collections::BTreeSet::from([WriteOperation::EntityRefMint]),
        valid_from: 1,
        valid_until: i64::MAX as u64,
        revision: 1,
        revoked: false,
        mint_scope: Some(MintScope {
            namespace: "fixture".into(),
            base_iri: config.namespaces[0].base_iri.clone(),
            types: std::collections::BTreeSet::from(["urn:example:Host".into()]),
        }),
    };
    store.writer_grant_apply(&grant).unwrap();
    let before: String = store
        .conn
        .query_row(
            "SELECT payload FROM writer_grants WHERE id='fixture-grant'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    request.expected_registry_hash = registry_hash(&store).unwrap();
    assert!(remove(&mut store, &request).is_err());
    let after: String = store
        .conn
        .query_row(
            "SELECT payload FROM writer_grants WHERE id='fixture-grant'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(list(&store).unwrap(), config.namespaces);
}

#[test]
fn request_requires_complete_selectors_and_refuses_unknown_fields() {
    let (_, _, request) = fixture();
    let mut json = serde_json::to_value(&request).unwrap();
    json["namespace"].as_object_mut().unwrap().remove("types");
    assert!(serde_json::from_value::<RemovalRequest>(json).is_err());
    let mut json = serde_json::to_value(&request).unwrap();
    json["force"] = serde_json::json!(true);
    assert!(serde_json::from_value::<RemovalRequest>(json).is_err());
}
