//! Machine-readable adapter for the W3C SHACL conformance harness.
//!
//! Default mode validates one data graph whole (`quipu::validate_shapes`).
//! `--write-path` (aegis-1c8q) instead drives the same data through the real
//! `/knot` write path in several splits, so a constraint the write gate judges
//! differently from whole-graph validation shows up as a divergence.

use std::collections::BTreeMap;
use std::env;
use std::fs;

use oxrdf::Triple;
use oxrdfio::{RdfFormat, RdfParser};
use serde_json::{Value, json};

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

/// The data graph as absolute N-Triples lines, parsed against the fixture's own
/// location so relative IRIs resolve the same way on every path.
fn triples(data: &str, base: &str) -> Vec<Triple> {
    RdfParser::from_format(RdfFormat::Turtle)
        .with_base_iri(base)
        .unwrap_or_else(|e| fail(format!("base IRI: {e}")))
        .for_reader(data.as_bytes())
        .map(|quad| {
            let quad = quad.unwrap_or_else(|e| fail(format!("data parse error: {e}")));
            Triple::new(quad.subject, quad.predicate, quad.object)
        })
        .collect()
}

fn ntriples(triples: &[&Triple]) -> String {
    let mut out = String::new();
    for triple in triples {
        out.push_str(&triple.to_string());
        out.push_str(" .\n");
    }
    out
}

/// One write through the real `/knot` handler. `conforms` is the gate's
/// verdict; any other refusal is reported separately as an error.
fn knot(store: &mut quipu::Store, shapes: &str, turtle: &str) -> Value {
    let input = json!({
        "turtle": turtle,
        "shapes": shapes,
        "timestamp": "2026-01-01T00:00:00Z",
    });
    match quipu::tool_knot(store, &input) {
        Ok(result) => json!({"accepted": result["conforms"] != json!(false)}),
        Err(error) => json!({"accepted": false, "error": error.to_string()}),
    }
}

/// The ordered chunks of one split strategy.
fn splits(all: &[Triple]) -> BTreeMap<&'static str, Vec<Vec<&Triple>>> {
    // Subjects in FIRST-APPEARANCE order, never sorted by their text: the parser
    // mints fresh labels for anonymous blank nodes on every run, so a text sort
    // made the split (and the gate) nondeterministic.
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    let mut subjects: Vec<Vec<&Triple>> = Vec::new();
    for triple in all {
        let next = subjects.len();
        let slot = *index.entry(triple.subject.to_string()).or_insert(next);
        if slot == next {
            subjects.push(Vec::new());
        }
        subjects[slot].push(triple);
    }
    let half = subjects.len().div_ceil(2);
    let first: Vec<&Triple> = subjects[..half].iter().flatten().copied().collect();
    let second: Vec<&Triple> = subjects[half..].iter().flatten().copied().collect();
    let mut out = BTreeMap::new();
    out.insert("single", vec![all.iter().collect()]);
    out.insert("subject_halves", vec![first.clone(), second.clone()]);
    out.insert("subject_halves_reversed", vec![second, first]);
    out.insert("per_subject", subjects.clone());
    out.insert("per_subject_reversed", subjects.into_iter().rev().collect());
    // One write per triple: the only split that puts two values of the SAME
    // focus node in different writes, which is what a cross-write cardinality
    // bug (PR697) needs. Subject-level splits keep a node's values together.
    out.insert("per_triple", all.iter().map(|t| vec![t]).collect());
    out.insert(
        "per_triple_reversed",
        all.iter().rev().map(|t| vec![t]).collect(),
    );
    out
}

fn write_path(shapes: &str, data: &str, base: &str) -> Value {
    let all = triples(data, base);
    let whole = ntriples(&all.iter().collect::<Vec<_>>());
    let batch = quipu::validate_shapes(shapes, &whole).map_or_else(
        |e| json!({"error": e.to_string()}),
        |report| json!({"conforms": report.conforms}),
    );
    let mut modes = serde_json::Map::new();
    for (name, chunks) in splits(&all) {
        let mut store =
            quipu::Store::open_in_memory().unwrap_or_else(|e| fail(format!("store: {e}")));
        let writes: Vec<Value> = chunks
            .iter()
            .filter(|chunk| !chunk.is_empty())
            .map(|chunk| knot(&mut store, shapes, &ntriples(chunk)))
            .collect();
        let conforms = writes.iter().all(|w| w["accepted"] == json!(true));
        modes.insert(
            name.to_string(),
            json!({"conforms": conforms, "writes": writes}),
        );
    }
    // Deliberately weak positive control: every triple validated ALONE, with no
    // store at all. It must keep diverging on a cross-write cardinality case even
    // after the real write gate is fixed, so the harness can always show that it
    // still detects a divergence (a run without it is UNKNOWN, not clean).
    let control = all.iter().all(|triple| {
        quipu::validate_shapes(shapes, &ntriples(&[triple])).is_ok_and(|report| report.conforms)
    });
    json!({
        "triples": all.len(),
        "batch": batch,
        "modes": modes,
        "control_payload_only": {"conforms": control},
    })
}

fn main() {
    let mut args = env::args().skip(1);
    let (mut shapes, mut data, mut base, mut write) = (None, None, None, false);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--shapes" => shapes = args.next(),
            "--data" => data = args.next(),
            "--base" => base = args.next(),
            "--write-path" => write = true,
            _ => fail(format!("unknown argument: {arg}")),
        }
    }
    let (Some(shapes), Some(data)) = (shapes, data) else {
        fail(
            "usage: quipu-shacl-conformance --shapes <ttl> --data <ttl> \
             [--write-path [--base <iri>]]",
        );
    };
    let base = base.unwrap_or_else(|| format!("file://{data}"));
    let shapes =
        fs::read_to_string(shapes).unwrap_or_else(|e| fail(format!("shapes read error: {e}")));
    let data = fs::read_to_string(data).unwrap_or_else(|e| fail(format!("data read error: {e}")));
    if write {
        println!("{}", write_path(&shapes, &data, &base));
        return;
    }
    match quipu::validate_shapes(&shapes, &data) {
        Ok(report) => println!(
            "{}",
            serde_json::to_string(&report).expect("serializable report")
        ),
        Err(error) => {
            eprintln!("validation error: {error}");
            std::process::exit(1);
        }
    }
}
