//! Both actual client serializers run against the native store, not a stub parser.
#![cfg(unix)]
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use proptest::prelude::*;
use quipu::Store;
use quipu::store::Datum;
use quipu::types::{Op, Value as RdfValue};
use serde_json::{Value, json};

const TS: &str = "2026-01-01T00:00:00Z";

fn emit(js: bool, literal: Option<&str>) -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let private = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(if js { "node" } else { "python3" });
    cmd.arg(root.join(if js {
        "tests/query-builder-cases.mjs"
    } else {
        "python/tests/builder_cases.py"
    }));
    if literal.is_some() {
        cmd.arg("--literal");
    } else if js {
        cmd.arg("--emit");
    }
    cmd.env("HOME", private.path())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(private.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    if let Some(value) = literal {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(value).unwrap())
            .unwrap();
    } else {
        drop(child.stdin.take());
    }
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn datum(store: &Store, s: &str, p: &str, value: RdfValue) -> Datum {
    Datum {
        entity: store.intern(s).unwrap(),
        attribute: store.intern(p).unwrap(),
        value,
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

#[test]
fn client_queries_agree_with_reference_and_execute_with_native_counts() {
    let fixture: Value =
        serde_json::from_str(include_str!("../test-fixtures/query-builder/cases.json")).unwrap();
    let python = emit(false, None);
    let js = emit(true, None);
    assert_eq!(python, js);
    let mut store = Store::open_in_memory().unwrap();
    let kind = store.intern("urn:Thing").unwrap();
    let b = store.intern("urn:b").unwrap();
    let data = vec![
        datum(&store, "urn:a", "urn:type", RdfValue::Ref(kind)),
        datum(&store, "urn:b", "urn:type", RdfValue::Ref(kind)),
        datum(&store, "urn:a", "urn:label", RdfValue::Str("Alpha".into())),
        datum(&store, "urn:b", "urn:label", RdfValue::Str("Beta".into())),
        datum(&store, "urn:a", "urn:link", RdfValue::Ref(b)),
        datum(&store, "urn:a", "urn:score", RdfValue::Int(1)),
        datum(&store, "urn:b", "urn:score", RdfValue::Int(2)),
    ];
    store.transact(&data, TS, None, None).unwrap();
    for (name, request) in python.as_object().unwrap() {
        assert_eq!(
            request["query"], fixture["queries"][name]["query"],
            "{name}"
        );
        let result = quipu::tool_query(&store, request).unwrap();
        assert_eq!(
            result["count"], fixture["queries"][name]["count"],
            "{name}: {result}"
        );
        // Save through the real validated/versioned registry and read it back
        // through the actual named-query consumer route.
        let stored_name = format!("builder_{name}");
        quipu::tool_queries(&store,&json!({"action":"load","name":stored_name,"template":request["query"],"params":[],"timestamp":TS})).unwrap();
        let asked = quipu::tool_ask(&store, &json!({"name":stored_name})).unwrap();
        assert_eq!(asked["count"], result["count"], "{name}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn unicode_literals_roundtrip_through_both_serializers_and_native_query(
        chars in prop::collection::vec(any::<char>(), 0..32)
    ) {
        let value: String=chars.into_iter().collect();
        let python=emit(false,Some(&value));
        let js=emit(true,Some(&value));
        prop_assert_eq!(&python,&js);
        let mut store=Store::open_in_memory().unwrap();
        let fact=datum(&store,"urn:literal","urn:value",RdfValue::Str(value.clone()));
        store.transact(&[fact],TS,None,None).unwrap();
        let result=quipu::tool_query(&store,&python).unwrap();
        prop_assert_eq!(&result["count"],&json!(1));
        prop_assert_eq!(&result["rows"][0]["value"],&json!(value));
    }
}

#[test]
fn literal_control_corpus_survives_registry_validation_and_named_execution() {
    let fixture: Value =
        serde_json::from_str(include_str!("../test-fixtures/query-builder/cases.json")).unwrap();
    for value in fixture["literals"].as_array().unwrap() {
        let value = value.as_str().unwrap();
        let python = emit(false, Some(value));
        assert_eq!(python, emit(true, Some(value)));
        let mut store = Store::open_in_memory().unwrap();
        let fact = datum(
            &store,
            "urn:literal",
            "urn:value",
            RdfValue::Str(value.into()),
        );
        store.transact(&[fact], TS, None, None).unwrap();
        quipu::tool_queries(&store, &json!({"action":"load","name":"literal_control","template":python["query"],"params":[],"timestamp":TS})).unwrap();
        let result = quipu::tool_ask(&store, &json!({"name":"literal_control"})).unwrap();
        assert_eq!(result["count"], 1, "{value:?}");
        assert_eq!(result["rows"][0]["value"], value);
    }
}
