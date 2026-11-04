//! Native runner for the shared bindings conformance corpus
//! (`tests/fixtures/bindings-conformance/v1.json`). The Python runner
//! (`bindings/python/tests/test_conformance.py`) executes the same file, so a
//! case that passes here and fails there is a binding divergence.
#![cfg(feature = "shacl")]

use std::collections::BTreeMap;

use quipu::bindings::Engine;
use serde_json::Value;

const CORPUS: &str = include_str!("fixtures/bindings-conformance/v1.json");

fn substitute(input: &Value, vars: &BTreeMap<String, Value>) -> Value {
    match input {
        Value::String(s) => s
            .strip_prefix("${")
            .and_then(|rest| rest.strip_suffix('}'))
            .map_or_else(
                || input.clone(),
                |name| {
                    vars.get(name)
                        .cloned()
                        .unwrap_or_else(|| panic!("unbound ${{{name}}}"))
                },
            ),
        Value::Array(items) => Value::Array(items.iter().map(|v| substitute(v, vars)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), substitute(v, vars)))
                .collect(),
        ),
        _ => input.clone(),
    }
}

fn matches(pattern: &Value, actual: &Value) -> bool {
    match pattern {
        Value::String(s) if s == "$positive_int" => actual.as_i64().is_some_and(|n| n > 0),
        Value::Object(p) if p.contains_key("$contains") => {
            let Some(items) = actual.as_array() else {
                return false;
            };
            p["$contains"]
                .as_array()
                .unwrap()
                .iter()
                .all(|want| items.iter().any(|item| matches(want, item)))
        }
        Value::Object(p) if p.contains_key("$len") => {
            actual.as_array().map(Vec::len) == p["$len"].as_u64().map(|n| n as usize)
        }
        Value::Object(p) => p
            .iter()
            .all(|(k, v)| actual.get(k).is_some_and(|a| matches(v, a))),
        Value::Array(p) => actual
            .as_array()
            .is_some_and(|a| a.len() == p.len() && p.iter().zip(a).all(|(p, a)| matches(p, a))),
        _ => pattern == actual,
    }
}

#[test]
fn the_shared_corpus_passes_natively() {
    let corpus: Value = serde_json::from_str(CORPUS).unwrap();
    assert_eq!(corpus["corpus"], "quipu-bindings-conformance/v1");
    let cases = corpus["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let mut engine = Engine::open_in_memory().unwrap();
        let mut vars: BTreeMap<String, Value> = corpus["shapes"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (format!("shape:{k}"), v.clone()))
            .collect();
        for (i, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            let op = step["op"].as_str().unwrap();
            let input = substitute(&step["input"], &vars);
            let result = engine.invoke(op, &input);
            let expect = &step["expect"];
            match (expect.get("ok"), expect.get("error"), result) {
                (Some(want), _, Ok(got)) => {
                    assert!(matches(want, &got), "{name}[{i}] {op}: {got}");
                    if let Some(capture) = step.get("capture").and_then(Value::as_object) {
                        for (var, pointer) in capture {
                            let value = got.pointer(pointer.as_str().unwrap()).unwrap();
                            vars.insert(var.clone(), value.clone());
                        }
                    }
                }
                (_, Some(want), Err(err)) => {
                    let got = serde_json::to_value(&err).unwrap();
                    assert!(matches(want, &got), "{name}[{i}] {op}: {got}");
                }
                (_, _, other) => panic!("{name}[{i}] {op}: expected {expect}, got {other:?}"),
            }
        }
    }
}
