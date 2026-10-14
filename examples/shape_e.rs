//! Arm E: research orchestration of two real share merges, shapes before data.
//! Named shape nodes only; this is not a deployed schema-negotiation protocol.

use std::collections::BTreeSet;
use std::path::Path;

use oxrdfio::{RdfFormat, RdfParser};
use quipu::Validator;
use quipu::rdf::{export_rdf_subset, ingest_rdf};
use quipu::share::{ShareDestination, ShareOptions, share};
use quipu::share_merge::{DecisionRecord, merge};
use quipu::store::Store;
use serde_json::json;

const NS: &str = "https://example.org/schema-bench/";
const SH: &str = "http://www.w3.org/ns/shacl#";
const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
const TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

type Graph = BTreeSet<(String, String, String)>;

fn iri(s: &str, p: &str, o: &str) -> String {
    format!("<{s}> <{p}> <{o}> .\n")
}

fn value(s: &str, p: &str, text: &str, datatype: &str) -> String {
    format!("<{s}> <{p}> \"{text}\"^^<{XSD}{datatype}> .\n")
}

fn graph(input: &str) -> Graph {
    RdfParser::from_format(RdfFormat::NTriples)
        .for_reader(input.as_bytes())
        .map(|quad| {
            let q = quad.expect("valid generated N-Triples");
            (
                q.subject.to_string(),
                q.predicate.to_string(),
                q.object.to_string(),
            )
        })
        .collect()
}

fn render(g: &Graph) -> String {
    g.iter()
        .map(|(s, p, o)| format!("{s} {p} {o} .\n"))
        .collect()
}

fn store(data: &str, shapes: &str) -> Store {
    let mut store = Store::open_in_memory().expect("isolated store");
    store
        .load_shapes("experiment", shapes, "2026-10-13")
        .expect("shapes");
    ingest_rdf(
        &mut store,
        data.as_bytes(),
        RdfFormat::NTriples,
        None,
        "2026-10-13T00:00:00Z",
        None,
        Some("synthetic-fixture"),
    )
    .expect("fixture graph");
    store
}

fn reconnect(
    base: &str,
    ours: &str,
    theirs: &str,
    shapes: &str,
    dir: &Path,
) -> (Graph, Vec<DecisionRecord>) {
    std::fs::create_dir_all(dir).expect("fixture directory");
    let source = store(base, shapes);
    let options = ShareOptions {
        shapes: vec!["experiment".into()],
        // These bundles never leave the run-scoped local fixture. Public
        // evidence is synthetic text, separately covered by the artifact gate.
        destination: ShareDestination::Internal,
        ..Default::default()
    };
    let base_share =
        share(&source, dir.join("base").to_str().unwrap(), &options).expect("real base envelope");
    // Exact replacement graphs in separate stores: do not append edits to base.
    let source = store(theirs, shapes);
    let options = ShareOptions {
        parent_share: Some(base_share.share_id),
        ..options
    };
    let incoming = dir.join("incoming");
    share(&source, incoming.to_str().unwrap(), &options).expect("real incoming envelope");
    let mut local = store(ours, shapes);
    let result = merge(
        &mut local,
        &incoming,
        "2026-10-13T00:01:00Z",
        Some("schema-benchmark"),
    )
    .expect("actual library merge");
    let (bytes, _) = export_rdf_subset(&local, RdfFormat::NTriples, None).expect("merged export");
    let mut output = graph(std::str::from_utf8(&bytes).expect("UTF-8"));
    // The library blocks the whole merge on conflict. Normalize each tested
    // contended slot to base solely for Arm A-compatible automatic-output scoring.
    let base_graph = graph(base);
    for conflict in &result.conflicts {
        let predicate = format!("<{}>", conflict.predicate);
        output.retain(|(s, p, _)| *s != conflict.subject || *p != predicate);
        output.extend(
            base_graph
                .iter()
                .filter(|(s, p, _)| *s == conflict.subject && *p == predicate)
                .cloned(),
        );
    }
    (output, result.conflicts)
}

fn property(name: &str, path: &str, bound: Option<usize>, datatype: &str) -> String {
    let node = format!("{NS}{name}");
    let mut result = iri(&node, TYPE, &format!("{SH}PropertyShape"))
        + &iri(&node, &format!("{SH}path"), path)
        + &iri(&node, &format!("{SH}datatype"), &format!("{XSD}{datatype}"));
    if let Some(n) = bound {
        result += &value(&node, &format!("{SH}maxCount"), &n.to_string(), "integer");
    }
    result
}

fn shapes(bound: Option<usize>, datatype: &str, remove: bool, add: bool) -> String {
    let node = format!("{NS}EntityShape");
    // An unchanged, unbounded note contract keeps shares nonempty on removal.
    let mut result = iri(&format!("{NS}NoteShape"), TYPE, &format!("{SH}NodeShape"))
        + &iri(
            &format!("{NS}NoteShape"),
            &format!("{SH}targetClass"),
            &format!("{NS}Entity"),
        )
        + &iri(
            &format!("{NS}NoteShape"),
            &format!("{SH}property"),
            &format!("{NS}NoteProp"),
        )
        + &property("NoteProp", &format!("{NS}note"), None, "string");
    if !remove {
        result += &(iri(&node, TYPE, &format!("{SH}NodeShape"))
            + &iri(&node, &format!("{SH}targetClass"), &format!("{NS}Entity"))
            + &iri(&node, &format!("{SH}property"), &format!("{NS}ValueProp"))
            + &property("ValueProp", &format!("{NS}value"), bound, datatype));
        if add {
            result += &(iri(&node, &format!("{SH}property"), &format!("{NS}TagProp"))
                + &property("TagProp", &format!("{NS}tag"), Some(1), "string"));
        }
    }
    result
}

fn meta() -> String {
    let mut result = format!("@prefix sh: <{SH}> . @prefix xsd: <{XSD}> .\n");
    result += "<https://example.org/schema-bench/Meta> a sh:NodeShape; \
               sh:targetClass sh:PropertyShape; \
               sh:property [ sh:path sh:path; sh:maxCount 1 ]; \
               sh:property [ sh:path sh:maxCount; sh:maxCount 1; sh:datatype xsd:integer ]; \
               sh:property [ sh:path sh:datatype; sh:maxCount 1 ].\n";
    result
}

fn violations(shapes: &str, data: &Graph) -> usize {
    Validator::from_turtle(shapes)
        .expect("validator contract")
        .validate(render(data).as_bytes())
        .expect("actual validation")
        .violations
}

fn context_questions(base: &Graph, ours: &Graph, theirs: &Graph) -> usize {
    // Context overlap over the synthetic data's changed subjects. All edited
    // objects are literals; there are no changed object-node endpoints here.
    let changes = |g: &Graph| -> Graph { base.symmetric_difference(g).cloned().collect() };
    let left = changes(ours);
    let right = changes(theirs);
    let subjects = |g: &Graph| -> BTreeSet<String> { g.iter().map(|t| t.0.clone()).collect() };
    let a = subjects(&left);
    let b = subjects(&right);
    left.union(&right)
        .filter(|(s, _, _)| a.contains(s) && b.contains(s))
        .map(|(s, p, _)| (s, p))
        .collect::<BTreeSet<_>>()
        .len()
}

fn run(seed: u64, kind: &str, root: &Path) -> serde_json::Value {
    let entity = format!("{NS}entity-{seed}");
    let p = format!("{NS}value");
    let tag = format!("{NS}tag");
    let type_triple = iri(&entity, TYPE, &format!("{NS}Entity"));
    let base_value = value(&entity, &p, "base", "string");
    let left_value = value(&entity, &p, &format!("left-{seed}"), "string");
    let right_value = value(&entity, &p, &format!("right-{seed}"), "string");
    let (sb, sr, base, left, right, expected, truth) = match kind {
        "tighten" => (
            shapes(Some(2), "string", false, false),
            shapes(Some(1), "string", false, false),
            type_triple.clone() + &base_value,
            type_triple.clone() + &base_value + &left_value,
            type_triple.clone() + &right_value,
            type_triple.clone() + &base_value,
            vec![p.clone()],
        ),
        "loosen" => (
            shapes(Some(1), "string", false, false),
            shapes(None, "string", false, false),
            type_triple.clone() + &base_value,
            type_triple.clone() + &left_value,
            type_triple.clone() + &right_value,
            type_triple.clone() + &left_value + &right_value,
            vec![],
        ),
        "property-added" => (
            shapes(Some(2), "string", false, false),
            shapes(Some(2), "string", false, true),
            type_triple.clone() + &base_value,
            type_triple.clone() + &base_value + &value(&entity, &tag, "left", "string"),
            type_triple.clone() + &base_value + &value(&entity, &tag, "right", "string"),
            type_triple.clone() + &base_value,
            vec![tag],
        ),
        "datatype" => (
            shapes(Some(2), "string", false, false),
            shapes(Some(2), "integer", false, false),
            type_triple.clone(),
            type_triple.clone() + &left_value,
            type_triple.clone() + &value(&entity, &p, "7", "integer"),
            type_triple.clone(),
            vec![p.clone()],
        ),
        "removed" => (
            shapes(Some(1), "string", false, false),
            shapes(None, "string", true, false),
            type_triple.clone() + &base_value,
            type_triple.clone() + &left_value,
            type_triple.clone() + &right_value,
            type_triple + &left_value + &right_value,
            vec![],
        ),
        _ => unreachable!(),
    };
    assert_eq!(violations(&sb, &graph(&base)), 0);
    assert_eq!(violations(&sb, &graph(&left)), 0);
    assert_eq!(violations(&sr, &graph(&right)), 0);
    let dir = root.join(format!("{seed}-{kind}"));
    let (merged_shapes, schema_conflicts) = reconnect(&sb, &sb, &sr, &meta(), &dir.join("shapes"));
    assert!(schema_conflicts.is_empty());
    assert_eq!(merged_shapes, graph(&sr));
    assert_eq!(violations(&meta(), &merged_shapes), 0);
    let contract = render(&merged_shapes);
    let (actual, decisions) = reconnect(&base, &left, &right, &contract, &dir.join("data"));
    let found: BTreeSet<_> = decisions.iter().map(|d| d.predicate.clone()).collect();
    let oracle: BTreeSet<_> = truth.into_iter().collect();
    let target = graph(&expected);
    let invalid = violations(&contract, &actual);
    json!({"seed": seed, "case": kind, "schema_conflicts": schema_conflicts.len(),
        "tp": found.intersection(&oracle).count(), "fp": found.difference(&oracle).count(),
        "fn": oracle.difference(&found).count(), "data_questions": decisions.len(),
        "context_questions": context_questions(&graph(&base), &graph(&left), &graph(&right)),
        "triples_lost": target.difference(&actual).count(),
        "triples_extra": actual.difference(&target).count(), "shacl_violations": invalid,
        "base_shapes": sb, "left_shapes": sb, "right_shapes": sr,
        "base_data": base, "left_data": left, "right_data": right,
        "oracle_output": expected, "merged_shapes": contract, "automatic_output": render(&actual)})
}

fn main() {
    let tmp = tempfile::tempdir().expect("run-scoped fixture");
    let old = shapes(Some(1), "string", false, false);
    let left = shapes(Some(2), "string", false, false);
    let right = shapes(Some(3), "string", false, false);
    let (held, conflicts) = reconnect(&old, &left, &right, &meta(), &tmp.path().join("control"));
    assert_eq!(conflicts.len(), 1, "meta-shape conflict positive control");
    assert_eq!(held, graph(&old), "conflicted meta-slot held at base");
    assert_eq!(violations(&meta(), &held), 0);
    let dirty = graph(
        &(old.clone()
            + &value(
                &format!("{NS}ValueProp"),
                &format!("{SH}maxCount"),
                "2",
                "integer",
            )),
    );
    assert!(
        violations(&meta(), &dirty) > 0,
        "meta-validator positive control"
    );
    let mut results = Vec::new();
    for seed in [1, 7, 42, 99, 2026] {
        for kind in ["tighten", "loosen", "property-added", "datatype", "removed"] {
            let result = run(seed, kind, tmp.path());
            if kind == "datatype" {
                assert_eq!(result["fn"], 1);
                assert!(result["shacl_violations"].as_u64().unwrap() > 0);
            } else {
                assert_eq!(result["fn"], 0);
                assert_eq!(result["shacl_violations"], 0);
            }
            assert_eq!(result["fp"], 0);
            results.push(result);
        }
    }
    if std::env::args().any(|arg| arg == "--selftest") {
        println!(
            "PASS: meta conflict/validation controls, legal branch controls, five seeds, datatype blind spot observed"
        );
    } else {
        println!("{}", serde_json::to_string_pretty(&results).expect("JSON"));
    }
}
