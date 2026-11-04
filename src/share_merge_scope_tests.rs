//! Three-way rule and target-scoped `sh:maxCount` bounds for share merge
//! (review of #888): an untouched over-bound slot never conflicts, a bound
//! applies only to the focus nodes its shape targets, and emit-mode shapes
//! do not gate.

use std::collections::BTreeSet;

use super::{Graph, by_slot, merge_graphs, parse_graph};

const P_ONE: &str = "https://example.org/one";
const P_MANY: &str = "https://example.org/many";
const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
[] sh:path <https://example.org/one> ; sh:maxCount 1 .
[] sh:path <https://example.org/many> .
"#;

fn graph(lines: &[(&str, &str)]) -> Graph {
    let text = lines
        .iter()
        .map(|(p, o)| format!("<https://example.org/s> <{p}> {o} .\n"))
        .collect::<String>();
    parse_graph(&text, "test graph").unwrap()
}

fn triples(lines: &[(&str, &str, &str)]) -> Graph {
    let text = lines
        .iter()
        .map(|(s, p, o)| format!("<https://example.org/{s}> <{p}> {o} .\n"))
        .collect::<String>();
    parse_graph(&text, "test graph").unwrap()
}

const TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const SUB: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const TARGETED: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <https://example.org/> .
ex:NamedShape a sh:NodeShape ; sh:targetClass ex:Named ;
sh:property [ sh:path ex:one ; sh:maxCount 1 ] .
"#;

#[test]
fn an_over_bound_slot_neither_side_touched_is_not_a_conflict() {
    // Pre-existing data over the bound must not block unrelated merges.
    let base = triples(&[("s", P_ONE, "\"a\""), ("s", P_ONE, "\"b\"")]);
    let mut theirs = base.clone();
    theirs.extend(triples(&[("s", P_MANY, "\"new\"")]));
    let (merged, conflicts) = merge_graphs(&base, &base, &theirs, SHAPES).unwrap();
    assert!(conflicts.is_empty(), "{conflicts:?}");
    assert_eq!(merged, theirs);
}

#[test]
fn a_slot_only_one_side_changed_takes_that_side() {
    let base = graph(&[(P_ONE, "\"base\"")]);
    let theirs = graph(&[(P_ONE, "\"x\""), (P_ONE, "\"y\"")]);
    let (merged, conflicts) = merge_graphs(&base, &base, &theirs, SHAPES).unwrap();
    assert!(conflicts.is_empty(), "{conflicts:?}");
    assert_eq!(merged, theirs);
}

#[test]
fn max_count_applies_only_to_the_shapes_target_class() {
    let typed = ("s", TYPE, "<https://example.org/Named>");
    let base = triples(&[typed, ("s", P_ONE, "\"base\""), ("u", P_ONE, "\"base\"")]);
    let ours = triples(&[typed, ("s", P_ONE, "\"ours\""), ("u", P_ONE, "\"ours\"")]);
    let theirs = triples(&[
        typed,
        ("s", P_ONE, "\"theirs\""),
        ("u", P_ONE, "\"theirs\""),
    ]);
    let (merged, conflicts) = merge_graphs(&base, &ours, &theirs, TARGETED).unwrap();
    // s is a Named: held at base and reported. u is outside the target:
    // both edits survive, as for any unbounded slot.
    assert_eq!(conflicts.len(), 1, "{conflicts:?}");
    assert_eq!(conflicts[0].subject, "<https://example.org/s>");
    let u = by_slot(&merged)
        .get(&("<https://example.org/u>".into(), P_ONE.into()))
        .cloned()
        .unwrap_or_default();
    assert_eq!(u, BTreeSet::from(["\"ours\"".into(), "\"theirs\"".into()]));
}

#[test]
fn a_subclass_instance_is_targeted() {
    let schema = ("Special", SUB, "<https://example.org/Named>");
    let typed = ("s", TYPE, "<https://example.org/Special>");
    let base = triples(&[schema, typed, ("s", P_ONE, "\"base\"")]);
    let ours = triples(&[schema, typed, ("s", P_ONE, "\"ours\"")]);
    let theirs = triples(&[schema, typed, ("s", P_ONE, "\"theirs\"")]);
    let (_, conflicts) = merge_graphs(&base, &ours, &theirs, TARGETED).unwrap();
    assert_eq!(conflicts.len(), 1, "{conflicts:?}");
}

#[test]
fn emit_mode_shapes_do_not_gate_the_merge() {
    let emit = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <https://example.org/> .
@prefix quipu: <http://quipu.dev/ontology/> .
ex:Observe a sh:NodeShape ; quipu:onViolation "emit" ;
sh:targetSubjectsOf ex:one ;
sh:property [ sh:path ex:one ; sh:maxCount 1 ] .
"#;
    let base = graph(&[(P_ONE, "\"base\"")]);
    let ours = graph(&[(P_ONE, "\"ours\"")]);
    let theirs = graph(&[(P_ONE, "\"theirs\"")]);
    let (_, conflicts) = merge_graphs(&base, &ours, &theirs, emit).unwrap();
    assert!(conflicts.is_empty(), "{conflicts:?}");
    let reject = emit.replace(r#"quipu:onViolation "emit" ;"#, "");
    let (_, conflicts) = merge_graphs(&base, &ours, &theirs, &reject).unwrap();
    assert_eq!(conflicts.len(), 1, "the same shape without emit must gate");
}
