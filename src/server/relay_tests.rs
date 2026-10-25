//! The relay's SSE parsing: events split across chunks arrive whole.
use super::SseParser;

#[test]
fn events_split_across_chunks_arrive_whole_and_in_order() {
    let mut parser = SseParser::default();
    assert!(
        parser
            .push("event: quipu.replicate.tx\ndata: {\"a\"")
            .is_empty()
    );
    let events = parser.push(":1}\n\nevent: quipu.replicate.coverage\ndata: {}\n\n:keep\n\n");
    assert_eq!(
        events,
        vec![
            ("quipu.replicate.tx".to_string(), "{\"a\":1}".to_string()),
            ("quipu.replicate.coverage".to_string(), "{}".to_string()),
        ]
    );
}

#[test]
fn a_multi_line_data_field_is_joined_with_newlines() {
    let mut parser = SseParser::default();
    let events = parser.push("event: e\ndata: one\ndata: two\n\n");
    assert_eq!(events, vec![("e".to_string(), "one\ntwo".to_string())]);
}

fn relay_config(key: &str) -> quipu::config::RelayConfig {
    serde_json::from_value(serde_json::json!({
        "upstream_url": "http://upstream.invalid", "upstream_store_id": "s",
        "upstream_key": key, "graphs": ["urn:graph:board"],
    }))
    .unwrap()
}

fn signing(store: &mut quipu::Store) {
    use ring::signature::Ed25519KeyPair;
    let doc = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    let identity = quipu::signing::SigningIdentity::new(
        Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap(),
        "quipu",
    );
    store.set_signing_identity(std::sync::Arc::new(identity));
}

#[test]
fn a_relay_starts_only_on_an_empty_signing_store_with_a_pinned_key() {
    let key = "ab".repeat(32);
    let mut empty = quipu::Store::open_in_memory().unwrap();
    signing(&mut empty);
    assert!(super::open(&empty, &relay_config(&key)).is_ok());

    // No signing key: it could not sign its children's coverage.
    let unsigned = quipu::Store::open_in_memory().unwrap();
    assert!(super::open(&unsigned, &relay_config(&key)).is_err());

    // A malformed pinned key would never verify upstream coverage.
    assert!(super::open(&empty, &relay_config("not-a-key")).is_err());

    // Local facts: the relay would serve transactions with no source.
    let mut local = quipu::Store::open_in_memory().unwrap();
    signing(&mut local);
    let datum = quipu::store::Datum {
        entity: local.intern("urn:item:1").unwrap(),
        attribute: local.intern("urn:p:title").unwrap(),
        value: quipu::types::Value::Str("one".into()),
        valid_from: "2026-10-24T00:00:00Z".into(),
        valid_to: None,
        op: quipu::types::Op::Assert,
    };
    local
        .transact(&[datum], "2026-10-24T00:00:00Z", None, None)
        .unwrap();
    assert!(super::open(&local, &relay_config(&key)).is_err());
}
