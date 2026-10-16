//! `quipu validate --format json` must carry the full result multiset that the
//! text form drops (source shape, component, value, warning detail), with the
//! same exit status, so two reports can be compared exactly (aegis-g3j).
#![cfg(feature = "shacl")]

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};

const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix ex: <http://example.org/> .

ex:NameShape a sh:NodeShape ;
    sh:targetClass ex:Person ;
    sh:property [ sh:path ex:name ; sh:datatype xsd:string ; sh:maxCount 1 ] .

ex:SingleNameShape a sh:NodeShape ;
    sh:targetClass ex:Person ;
    sh:property [ sh:path ex:name ; sh:maxCount 1 ] .

ex:NickShape a sh:NodeShape ;
    sh:targetClass ex:Person ;
    sh:severity sh:Warning ;
    sh:property [ sh:path ex:nick ; sh:minCount 1 ; sh:severity sh:Warning ] .
"#;

// ex:a breaks maxCount in BOTH name shapes (same focus, path and component,
// different source shapes) and has a non-string name; ex:b only lacks a nick.
const BAD: &str = r#"
@prefix ex: <http://example.org/> .
ex:a a ex:Person ; ex:name "one", 2 ; ex:nick "a" .
ex:b a ex:Person ; ex:name "bee" .
"#;

const WARN_ONLY: &str = r#"
@prefix ex: <http://example.org/> .
ex:b a ex:Person ; ex:name "bee" .
"#;

const GOOD: &str = r#"
@prefix ex: <http://example.org/> .
ex:c a ex:Person ; ex:name "sea" ; ex:nick "c" .
"#;

fn run(dir: &Path, data: &str, format: Option<&str>) -> Output {
    let shapes = dir.join("shapes.ttl");
    let data_path = dir.join("data.ttl");
    std::fs::write(&shapes, SHAPES).unwrap();
    std::fs::write(&data_path, data).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_quipu"));
    cmd.arg("validate")
        .arg("--shapes")
        .arg(&shapes)
        .arg("--data")
        .arg(&data_path);
    if let Some(format) = format {
        cmd.args(["--format", format]);
    }
    cmd.output().unwrap()
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).expect("stdout is one JSON document")
}

fn field(result: &serde_json::Value, key: &str) -> String {
    result[key].as_str().unwrap_or("").to_string()
}

/// Multiset of (focus, message, path), the projection the text form prints.
fn projection_of_json(report: &serde_json::Value) -> BTreeMap<(String, String, String), usize> {
    let mut set = BTreeMap::new();
    for r in report["results"].as_array().unwrap() {
        let message = r["message"]
            .as_str()
            .unwrap_or("constraint violated")
            .to_string();
        *set.entry((field(r, "focus_node"), message, field(r, "path")))
            .or_default() += 1;
    }
    set
}

fn projection_of_text(stdout: &[u8]) -> BTreeMap<(String, String, String), usize> {
    let mut set = BTreeMap::new();
    for line in String::from_utf8_lossy(stdout).lines().skip(1) {
        let (_, rest) = line.split_once("] ").expect("result line");
        let (focus, rest) = rest.split_once(" -- ").expect("focus separator");
        let (message, path) = match rest.rsplit_once(" (path: ") {
            Some((m, p)) => (m.to_string(), p.trim_end_matches(')').to_string()),
            None => (rest.to_string(), String::new()),
        };
        *set.entry((focus.to_string(), message, path)).or_default() += 1;
    }
    set
}

#[test]
fn json_keeps_every_result_with_shape_component_and_value() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), BAD, Some("json"));
    assert_eq!(out.status.code(), Some(1), "non-conforming data exits 1");
    let report = json(&out);
    let results = report["results"].as_array().unwrap();
    assert_eq!(report["conforms"], false);
    assert_eq!(
        results.len() as u64,
        report["violations"].as_u64().unwrap() + report["warnings"].as_u64().unwrap()
    );
    for r in results {
        assert!(
            !field(r, "source_shape").is_empty(),
            "source shape kept: {r}"
        );
        assert!(!field(r, "component").is_empty(), "component kept: {r}");
    }
    // Same focus, path and component from two different shapes: both stay.
    let max_count: Vec<_> = results
        .iter()
        .filter(|r| field(r, "focus_node").ends_with("/a"))
        .filter(|r| field(r, "component").ends_with("MaxCountConstraintComponent"))
        .collect();
    assert_eq!(
        max_count.len(),
        2,
        "no dedup across source shapes: {results:?}"
    );
    assert!(
        results.iter().any(
            |r| field(r, "component").ends_with("DatatypeConstraintComponent")
                && !field(r, "value").is_empty()
        ),
        "offending value kept: {results:?}"
    );
}

#[test]
fn json_and_text_agree_on_the_projection_they_share() {
    let dir = tempfile::tempdir().unwrap();
    let report = json(&run(dir.path(), BAD, Some("json")));
    let text = run(dir.path(), BAD, None);
    assert_eq!(text.status.code(), Some(1));
    assert_eq!(
        projection_of_json(&report),
        projection_of_text(&text.stdout)
    );

    // Negative control: the comparison must see a removed fault.
    let warn = json(&run(dir.path(), WARN_ONLY, Some("json")));
    assert_ne!(projection_of_json(&warn), projection_of_json(&report));
}

#[test]
fn json_reports_warning_detail_and_conforming_data() {
    let dir = tempfile::tempdir().unwrap();
    let warn = run(dir.path(), WARN_ONLY, Some("json"));
    assert_eq!(
        warn.status.code(),
        Some(1),
        "a lone warning is not sh:conforms"
    );
    let report = json(&warn);
    assert_eq!(
        (report["violations"].as_u64(), report["warnings"].as_u64()),
        (Some(0), Some(1))
    );
    assert_eq!(field(&report["results"][0], "severity"), "Warning");

    let good = run(dir.path(), GOOD, Some("json"));
    assert_eq!(good.status.code(), Some(0));
    let report = json(&good);
    assert_eq!(report["conforms"], true);
    assert!(report["results"].as_array().unwrap().is_empty());
}

#[test]
fn text_is_still_the_default_and_unknown_formats_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let good = run(dir.path(), GOOD, None);
    assert_eq!(good.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&good.stdout),
        "valid (0 warnings)\n"
    );

    let refused = run(dir.path(), GOOD, Some("yaml"));
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty(), "no report on a refused format");
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--format text|json"));
}
