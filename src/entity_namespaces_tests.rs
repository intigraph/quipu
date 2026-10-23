use super::*;
use serde_json::json;

pub(crate) fn config() -> crate::QuipuConfig {
    let mut config = crate::QuipuConfig::default();
    config.namespaces.push(Namespace {
        prefix: "pub".into(),
        base_iri: "https://public.example/id/".into(),
        ..Namespace::default()
    });
    config
}

#[test]
fn public_defaults_and_invalid_configuration() {
    let defaults = Namespace::default();
    assert_eq!(defaults.base_iri, "https://w3id.org/intigraph/");
    assert_eq!(defaults.audience, "public");
    let mut store = Store::open_in_memory().unwrap();
    let mut cfg = config();
    configure(&mut store, &cfg).unwrap();
    cfg.namespaces[0].visibility = "private".into();
    assert!(configure(&mut store, &cfg).is_err());
    cfg.namespaces[0].audience = "org:team".into();
    configure(&mut store, &cfg).unwrap();
    for bad in [
        "relative/",
        "https://u:p@example.test/",
        "https://example.test/?x=1",
        "https://example.test/#thing/",
    ] {
        cfg.namespaces[0].base_iri = bad.into();
        assert!(configure(&mut store, &cfg).is_err(), "{bad}");
    }
}

#[test]
fn registry_survives_reopen_and_does_not_declassify_when_config_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.db");
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    let mut cfg = config();
    cfg.namespaces[0].visibility = "private".into();
    cfg.namespaces[0].audience = "org:team".into();
    configure(&mut store, &cfg).unwrap();
    let before = store.current_facts().unwrap().len();
    crate::entity_reference::tool_namespaces(&store, &json!({})).unwrap();
    assert_eq!(before, store.current_facts().unwrap().len());
    assert!(store.lookup(REGISTRY_GRAPH).unwrap().is_none());
    apply(&mut store).unwrap();
    apply(&mut store).unwrap();
    drop(store);
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    assert_eq!(list(&store).unwrap(), cfg.namespaces);
    configure(&mut store, &crate::QuipuConfig::default()).unwrap();
    assert_eq!(list(&store).unwrap()[0].audience, "org:team");
    cfg.namespaces[0].visibility = "public".into();
    cfg.namespaces[0].audience = "public".into();
    assert!(configure(&mut store, &cfg).is_err());
}

#[test]
fn opaque_mint_selectors_legacy_and_exact_reference() {
    let mut store = Store::open_in_memory().unwrap();
    let mut cfg = config();
    cfg.namespaces[0].types = vec!["urn:example:Host".into()];
    cfg.namespaces[0].graphs = vec!["urn:quipu:graph:root".into()];
    cfg.namespaces[0].resolver_url = Some("https://resolver.example/entity?iri={iri}".into());
    configure(&mut store, &cfg).unwrap();
    let input =
        json!({"action":"mint","namespace":"pub","type":"urn:example:Host","label":"renamable"});
    assert!(crate::entity_reference::tool_entity_ref(&mut store, &input).is_err());
    apply(&mut store).unwrap();
    let a = crate::entity_reference::tool_entity_ref(&mut store, &input).unwrap();
    let b = crate::entity_reference::tool_entity_ref(&mut store, &input).unwrap();
    assert_ne!(a["iri"], b["iri"]);
    let iri = a["iri"].as_str().unwrap();
    let key = iri.strip_prefix(&cfg.namespaces[0].base_iri).unwrap();
    assert_eq!(key.len(), 26);
    assert!(
        key.bytes()
            .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
    );
    assert!(
        crate::entity_reference::tool_entity_ref(
            &mut store,
            &json!({"action":"mint","namespace":"unknown","type":"urn:example:Host"})
        )
        .is_err()
    );
    assert!(
        crate::entity_reference::tool_entity_ref(
            &mut store,
            &json!({"action":"mint","namespace":"pub","type":"urn:wrong"})
        )
        .is_err()
    );
    assert!(crate::entity_reference::tool_entity_ref(&mut store,&json!({"action":"mint","namespace":"pub","type":"urn:example:Host","graph":"urn:wrong"})).is_err());
    let reference = crate::entity_reference::tool_entity_ref(
        &mut store,
        &json!({"action":"resolve","iri":iri,"source":"pendant-v1","asOf":"2026-10-20T00:00:00Z"}),
    )
    .unwrap();
    assert_eq!(reference["iri"], iri);
    assert_eq!(reference["known"], true);
    assert!(
        reference["resolver_url"]
            .as_str()
            .unwrap()
            .contains("&asOf=")
    );
    assert_eq!(store.base_ns(), crate::namespace::DEFAULT_BASE_NS);
}

#[test]
fn label_keys_never_recycle_and_unknown_fields_refuse() {
    let mut store = Store::open_in_memory().unwrap();
    let mut cfg = config();
    cfg.namespaces[0].minting = "label".into();
    configure(&mut store, &cfg).unwrap();
    apply(&mut store).unwrap();
    let input = json!({"action":"mint","namespace":"pub","type":"urn:Host","label":"same"});
    crate::entity_reference::tool_entity_ref(&mut store, &input).unwrap();
    assert!(crate::entity_reference::tool_entity_ref(&mut store, &input).is_err());
    assert!(
        crate::entity_reference::tool_entity_ref(
            &mut store,
            &json!({"action":"mint","namespace":"pub","type":"urn:Host","label":"../escape"})
        )
        .is_err()
    );
    assert!(crate::entity_reference::tool_namespaces(&store, &json!({"action":"apply"})).is_err());
    assert!(
        crate::entity_reference::tool_entity_ref(
            &mut store,
            &json!({"action":"resolve","iri":"https://public.example/id/same","asOf":"yesterday"})
        )
        .is_err()
    );
}
