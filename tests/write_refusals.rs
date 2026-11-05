//! Actual isolated gate refusals reach both the counter and the existing event.
#![allow(clippy::unwrap_used)]
use std::sync::Arc;

use quipu::{Datum, Error, Op, Store, Value};
use serde_json::Value as Json;

const NOW: &str = "2026-01-01T00:00:00Z";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";

fn datum(store: &Store, entity: &str, attribute: &str, value: Value) -> Datum {
    Datum {
        entity: store.intern(entity).unwrap(),
        attribute: store.intern(attribute).unwrap(),
        value,
        valid_from: NOW.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

fn total(client: &str) -> u64 {
    quipu::metrics::metrics().write_refusal_stats()["series"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["client"] == client)
        .map(|row| row["count"].as_u64().unwrap())
        .sum()
}

fn request<T>(client: &str, f: impl FnOnce() -> T) -> T {
    let provenance =
        quipu::write_provenance::RequestProvenance::classify(client, "/episode", [None; 5]);
    quipu::write_provenance::scoped(Some(Arc::new(provenance)), f)
}

fn refusal_events(store: &Store) -> Vec<quipu::store::events::EventRow> {
    store
        .events_after(0, 100, Some(&["write.refused".into()]), None)
        .unwrap()
}

#[test]
fn refused_governance_write_counts_once_but_success_and_speculation_do_not() {
    let client = "policy-refusal-fixture";
    let mut store = Store::open_in_memory().unwrap();
    store.embedding_config_mut().auto_embed = false;
    let ns = quipu::namespace::DEFAULT_BASE_NS;
    store.governance_config_mut().enforce_on_write = true;
    let policy = vec![
        datum(
            &store,
            "https://example.test/policy",
            RDF_TYPE,
            Value::Ref(store.intern(&format!("{ns}Policy")).unwrap()),
        ),
        datum(
            &store,
            "https://example.test/policy",
            &format!("{ns}targets"),
            Value::Str("https://example.test/Document".into()),
        ),
        datum(
            &store,
            "https://example.test/policy",
            &format!("{ns}claim"),
            Value::Str(format!("ASK {{ $target <{LABEL}> ?label }}")),
        ),
        datum(
            &store,
            "https://example.test/policy",
            &format!("{ns}boundary"),
            Value::Str("action".into()),
        ),
        datum(
            &store,
            "https://example.test/policy",
            &format!("{ns}effect"),
            Value::Str("deny".into()),
        ),
    ];
    store.transact(&policy, NOW, None, None).unwrap();
    let bad = vec![datum(
        &store,
        "https://example.test/document",
        RDF_TYPE,
        Value::Ref(store.intern("https://example.test/Document").unwrap()),
    )];
    let before = total(client);
    request(client, || {
        let error = store
            .transact(&bad, NOW, Some("tester"), Some("fixture"))
            .unwrap_err();
        assert!(matches!(error, Error::PolicyDenied(_)));
        assert_eq!(total(client), before + 1);
        let after_refusal = total(client);
        let events = refusal_events(&store);
        assert_eq!(events.len(), 1);
        let payload: Json = serde_json::from_str(&events[0].payload).unwrap();
        assert_eq!(payload["reason_code"], "policy");
        assert_eq!(payload["client"], client);
        assert!(payload["shape"].is_null());
        assert!(!events[0].payload.contains("valid_from"));
        assert!(
            store
                .entity_facts(
                    store
                        .lookup("https://example.test/document")
                        .unwrap()
                        .unwrap()
                )
                .unwrap()
                .is_empty()
        );

        assert!(store.speculate(&bad, NOW, |_| Ok(())).is_err());
        assert_eq!(
            total(client),
            before + 1,
            "hypothetical denial is not counted"
        );
        assert_eq!(refusal_events(&store).len(), 1);

        let mut good = bad.clone();
        good.push(datum(
            &store,
            "https://example.test/document",
            LABEL,
            Value::Str("present".into()),
        ));
        store
            .transact(&good, NOW, Some("tester"), Some("fixture"))
            .unwrap();
        assert_eq!(
            total(client),
            before + 1,
            "a committed write is not a refusal"
        );
        assert_eq!(refusal_events(&store).len(), 1);
        println!(
            "REFUSAL_DEMO {}",
            serde_json::json!({
                "fixture": "governance", "before": before,
                "after_refusal": after_refusal, "after_success_and_speculation": total(client),
                "events": refusal_events(&store).len(), "reason": payload["reason_code"],
                "client": payload["client"],
            })
        );
    });
}

#[cfg(feature = "shacl")]
#[test]
fn invalid_episode_counts_one_write_not_two_violations_and_retains_shape() {
    let client = "shacl-refusal-fixture";
    let mut store = Store::open_in_memory().unwrap();
    store.embedding_config_mut().auto_embed = false;
    let base = "https://example.test/";
    store.set_base_ns(base);
    let shape = format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> .
         <{base}ServiceShape> a sh:NodeShape; sh:targetClass <{base}WebService>;
             sh:property <{base}NameShape> .
         <{base}NameShape> sh:path <{base}name>; sh:minCount 1 ."
    );
    let episode: quipu::episode::Episode = serde_json::from_value(serde_json::json!({
        "name": "bad-services", "shapes": shape,
        "nodes": [{"name":"first", "type":"WebService"}, {"name":"second", "type":"WebService"}],
        "edges": []
    }))
    .unwrap();
    let before = total(client);
    request(client, || {
        let error = quipu::episode::ingest_episode(&mut store, &episode, NOW, base).unwrap_err();
        assert!(matches!(
            error,
            Error::ValidationFailed { violations: 2, .. }
        ));
        assert_eq!(total(client), before + 1);
        assert!(store.current_facts().unwrap().is_empty());
        let events = refusal_events(&store);
        assert_eq!(events.len(), 1);
        let payload: Json = serde_json::from_str(&events[0].payload).unwrap();
        assert_eq!(payload["shape"], format!("{base}NameShape"));
        assert_eq!(payload["client"], client);
        println!(
            "REFUSAL_DEMO {}",
            serde_json::json!({
                "fixture": "two SHACL violations", "before": before, "after_refusal": total(client),
                "events": events.len(), "shape": payload["shape"], "client": payload["client"],
            })
        );
        let mut text = String::new();
        // Existing rendering entry point, used by the HTTP /metrics handler.
        text.push_str(&quipu::metrics::metrics().render(0, 0, 0, None));
        assert!(text.contains(
            "quipu_write_refusals_total{reason=\"shacl\",shape=\"https://example.test/NameShape\""
        ));
    });
}
