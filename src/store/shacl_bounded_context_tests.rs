//! A focus's stored post-state context is bounded by what the loaded shapes
//! read (aegis-7aee).
//!
//! A hot node (one repository named by every GitHub delivery) carried 44.6k
//! `prov:wasGeneratedBy` facts that no loaded shape constrains. Loading all of
//! them as context, then one type lookup per Ref value for `sh:class`, made each
//! small delivery hold the writer about 2.5 s. These tests pin the bound and
//! that narrowing never loses a violation the full context would catch.

use crate::Store;

fn knot(store: &mut Store, ttl: &str) -> crate::Result<serde_json::Value> {
    crate::tool_knot(
        store,
        &serde_json::json!({"turtle": format!(
            "@prefix ex: <http://ex.test/> . @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> . {ttl}"),
                            "timestamp": "2026-11-28T00:00:00Z"}),
    )
}

/// The shape of the GitHub set that matters here: targetClass shapes with
/// forward predicate paths, a maxCount, and an sh:class on the repository edge.
const REPO_SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://ex.test/> .
ex:RepoShape a sh:NodeShape ; sh:targetClass ex:Repo ;
    sh:property [ sh:path ex:name ; sh:maxCount 1 ] .
ex:ActivityShape a sh:NodeShape ; sh:targetClass ex:Activity ;
    sh:property [ sh:path ex:inRepo ; sh:maxCount 1 ; sh:class ex:Repo ] .
ex:PullRequestShape a sh:NodeShape ; sh:targetClass ex:PullRequest .";

/// The same, plus one closed shape: its reach is every predicate of a focus,
/// so the plan must fall back to the full context.
const CLOSED_SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://ex.test/> .
ex:RepoShape a sh:NodeShape ; sh:targetClass ex:Repo ;
    sh:property [ sh:path ex:name ; sh:maxCount 1 ] .
ex:ActivityShape a sh:NodeShape ; sh:targetClass ex:Activity ;
    sh:property [ sh:path ex:inRepo ; sh:maxCount 1 ; sh:class ex:Repo ] .
ex:ClosedShape a sh:NodeShape ; sh:targetClass ex:Sealed ; sh:closed true ;
    sh:property [ sh:path ex:only ] .";

/// A class-based target reached only through a data-graph subclass axiom
/// (aegis-tfcjd4 class-target arm). `ex:StepShape` only sanctions the class;
/// it constrains nothing, so a kind maxCount refusal can come only from
/// `ex:ActionShape` reached through `ex:Step rdfs:subClassOf ex:Action`.
const ACTION_SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://ex.test/> .
ex:ActionShape a sh:NodeShape ; sh:targetClass ex:Action ;
    sh:property [ sh:path ex:kind ; sh:maxCount 1 ] .
ex:StepShape a sh:NodeShape ; sh:targetClass ex:Step .";

/// The subclass arm again, where the VALUE context must exhaust its budget:
/// `ex:link` is constrained (`sh:class`), so its many refs stay in the base
/// context and each target's type is looked up in the value context. The
/// fallback then truncates the value context, so only the base-context subclass
/// walk can still carry `ex:Step rdfs:subClassOf ex:Action`.
const EXHAUSTING_ACTION_SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://ex.test/> .
ex:ActionShape a sh:NodeShape ; sh:targetClass ex:Action ;
    sh:property [ sh:path ex:kind ; sh:maxCount 1 ] ;
    sh:property [ sh:path ex:link ; sh:class ex:Thing ] .
ex:StepShape a sh:NodeShape ; sh:targetClass ex:Step .
ex:ThingShape a sh:NodeShape ; sh:targetClass ex:Thing .";

/// Base context stays under the 100k budget, base + value types goes over it.
const EXHAUSTING_LINKS: usize = 55_000;

const FAN: usize = 3_000;

fn hot_store(shapes: &str) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes("7aee", shapes, "2026-11-28T00:00:00Z")
        .unwrap();
    knot(&mut store, "ex:r a ex:Repo ; ex:name \"r\" .").unwrap();
    let fan: String = (0..FAN)
        .map(|i| format!("ex:r ex:generatedBy ex:ep{i} . "))
        .collect();
    knot(&mut store, &fan).unwrap();
    store
}

fn context_facts_of_one_write(
    store: &mut Store,
    ttl: &str,
) -> (crate::Result<serde_json::Value>, u64) {
    let before = store.poststate_profile.check_facts;
    let result = knot(store, ttl);
    (result, store.poststate_profile.check_facts - before)
}

fn violation(result: crate::Result<serde_json::Value>, component: &str) {
    // A refusal arrives either as an error (post-state gate) or as the knot
    // tool's non-conforming report; both mean nothing was written.
    match result {
        Err(crate::Error::ValidationFailed { messages, .. }) => assert!(
            messages.iter().any(|m| m.contains(component)),
            "expected {component}: {messages:?}"
        ),
        Ok(report) if report["conforms"] == serde_json::Value::Bool(false) => assert!(
            report["issues"]
                .as_array()
                .is_some_and(|issues| issues.iter().any(|i| i["component"]
                    .as_str()
                    .is_some_and(|c| c.contains(component)))),
            "expected {component}: {report}"
        ),
        other => panic!("expected a {component} refusal, got {other:?}"),
    }
}

fn absent(store: &Store, subject: &str) {
    let rows = crate::sparql::query(
        store,
        &format!("SELECT (COUNT(*) AS ?n) WHERE {{ <{subject}> ?p ?o }}"),
    )
    .unwrap();
    assert_eq!(
        rows.rows()[0].get("n"),
        Some(&crate::Value::Int(0)),
        "{subject} was written"
    );
}

#[test]
fn a_small_write_naming_a_hot_node_loads_only_the_constrained_facts() {
    let mut store = hot_store(REPO_SHAPES);
    // An activity that names the hot repository, as every GitHub delivery does.
    let (result, context) = context_facts_of_one_write(
        &mut store,
        "ex:a1 a ex:Activity ; ex:inRepo ex:r . ex:r ex:name \"r\" .",
    );
    result.unwrap();
    // Bounded: a handful of name/type/edge facts, never the 3,000-fact fan.
    assert!(
        context < 50,
        "context loaded {context} facts for a small write"
    );
}

#[test]
fn an_unboundable_shape_keeps_the_full_focus_context() {
    let mut store = hot_store(CLOSED_SHAPES);
    let (result, context) = context_facts_of_one_write(
        &mut store,
        "ex:a1 a ex:Activity ; ex:inRepo ex:r . ex:r ex:name \"r\" .",
    );
    result.unwrap();
    assert!(
        context >= FAN as u64,
        "sh:closed is not boundable: expected the full context, loaded {context}"
    );
}

#[test]
fn a_constrained_violation_on_the_hot_node_is_still_refused() {
    let mut store = hot_store(REPO_SHAPES);
    // The stored name is outside this payload; maxCount must still see it.
    violation(
        knot(&mut store, "ex:r ex:name \"renamed\" ."),
        "MaxCountConstraintComponent",
    );
}

#[test]
fn a_class_violation_against_a_stored_type_is_still_refused() {
    let mut store = hot_store(REPO_SHAPES);
    knot(&mut store, "ex:pr a ex:PullRequest .").unwrap();
    // The only violation needs the STORED type of the edge target.
    violation(
        knot(&mut store, "ex:a2 a ex:Activity ; ex:inRepo ex:pr ."),
        "ClassConstraintComponent",
    );
    absent(&store, "http://ex.test/a2");
}

#[test]
fn a_data_graph_subclass_still_reaches_a_class_target_when_context_is_bounded() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes("7aee-action", ACTION_SHAPES, "2026-11-28T00:00:00Z")
        .unwrap();
    knot(&mut store, "ex:Step rdfs:subClassOf ex:Action .").unwrap();
    knot(&mut store, "ex:s a ex:Step ; ex:kind \"a\" .").unwrap();
    let fan: String = (0..FAN)
        .map(|i| format!("ex:s ex:generatedBy ex:ep{i} . "))
        .collect();
    knot(&mut store, &fan).unwrap();
    // ex:s is an ex:Action only through the data-graph axiom.
    let (result, context) = context_facts_of_one_write(&mut store, "ex:s ex:kind \"b\" .");
    violation(result, "MaxCountConstraintComponent");
    assert!(context < 50, "context loaded {context} facts");
}

#[test]
fn the_plan_bounds_forward_paths_and_refuses_to_bound_a_closed_shape() {
    let plan = crate::shacl::Validator::from_turtle(REPO_SHAPES)
        .unwrap()
        .cardinality_plan()
        .unwrap();
    assert!(plan.subject_context_bounded);
    assert!(plan.subject_predicates.contains("http://ex.test/name"));
    assert!(plan.subject_predicates.contains("http://ex.test/inRepo"));
    assert!(
        !plan
            .subject_predicates
            .contains("http://ex.test/generatedBy")
    );
    let closed = crate::shacl::Validator::from_turtle(CLOSED_SHAPES)
        .unwrap()
        .cardinality_plan()
        .unwrap();
    assert!(!closed.subject_context_bounded);
}

#[test]
fn a_data_graph_subclass_still_reaches_a_class_target_when_the_value_context_is_exhausted() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes(
            "7aee-exhaust",
            EXHAUSTING_ACTION_SHAPES,
            "2026-11-28T00:00:00Z",
        )
        .unwrap();
    knot(&mut store, "ex:Step rdfs:subClassOf ex:Action .").unwrap();
    knot(&mut store, "ex:s a ex:Step ; ex:kind \"a\" .").unwrap();
    let things: String = (0..EXHAUSTING_LINKS)
        .map(|i| format!("ex:t{i} a ex:Thing . "))
        .collect();
    knot(&mut store, &things).unwrap();
    // In batches: one payload of 55k new refs would make every target a focus
    // and exhaust the BASE context of the setup write itself.
    for batch in (0..EXHAUSTING_LINKS).collect::<Vec<_>>().chunks(5_000) {
        let links: String = batch
            .iter()
            .map(|i| format!("ex:s ex:link ex:t{i} . "))
            .collect();
        knot(&mut store, &links).unwrap();
    }
    let fallbacks = store.poststate_profile.check_value_fallbacks;
    violation(
        knot(&mut store, "ex:s ex:kind \"b\" ."),
        "MaxCountConstraintComponent",
    );
    // The arm under test really ran: the value context fell back.
    assert!(
        store.poststate_profile.check_value_fallbacks > fallbacks,
        "the value context did not exhaust; this test proves nothing"
    );
}
