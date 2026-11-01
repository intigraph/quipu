//! Tests for the proposal blast-radius plan (aegis-493).

use super::*;
use crate::proposal::NewProposal;
use crate::store::queries::StoredQuery;

const T0: &str = "2026-01-01T00:00:00Z";
const EX: &str = "http://example.org/";

const PREFIXES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
     @prefix ex: <http://example.org/> .\n";

/// Five people: alice, bob and carol carry two emails, dave one, erin none.
const PEOPLE: &str = r#"
@prefix ex: <http://example.org/> .
ex:alice a ex:Person ; ex:email "a1" , "a2" .
ex:bob   a ex:Person ; ex:email "b1" , "b2" .
ex:carol a ex:Person ; ex:email "c1" , "c2" .
ex:dave  a ex:Person ; ex:email "d1" .
ex:erin  a ex:Person .
"#;

fn store_with(turtle: &str, graph: i64) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    ingest(&mut store, turtle, graph);
    store
}

fn ingest(store: &mut Store, turtle: &str, graph: i64) {
    crate::rdf::ingest_rdf_chunked(
        store,
        turtle.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        T0,
        Some("test"),
        Some("seed"),
        graph,
        10_000,
    )
    .unwrap();
}

/// A Person's emails: at least `min`, at most `max`.
fn email_shape(min: u32, max: u32) -> String {
    format!(
        "{PREFIXES}ex:PersonShape a sh:NodeShape ; sh:targetClass ex:Person ; \
         sh:property [ sh:path ex:email ; sh:minCount {min} ; sh:maxCount {max} ] ."
    )
}

fn propose(store: &Store, kind: &ProposalKind, diff: &str) -> i64 {
    store
        .insert_proposal(&NewProposal {
            kind,
            target: "people",
            diff,
            rationale: None,
            proposer: "agent/test",
            trigger_ref: None,
            created_at: T0,
        })
        .unwrap()
}

#[test]
fn terms_names_classes_and_properties_and_implicit_targets() {
    let t = terms(&format!(
        "{PREFIXES}@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
         ex:Pet a rdfs:Class , sh:NodeShape ; rdfs:subClassOf ex:Animal ; \
           sh:property [ sh:path ex:owner ; sh:class ex:Person ] .\n\
         ex:age rdfs:domain ex:Person ; rdfs:range <http://www.w3.org/2001/XMLSchema#int> ."
    ))
    .unwrap();
    let iris = |v: &[&str]| {
        v.iter()
            .map(|l| format!("{EX}{l}"))
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(t.classes, iris(&["Animal", "Person", "Pet"]));
    assert_eq!(t.properties, iris(&["age", "owner"]));
    assert_eq!(t.implicit_targets, iris(&["Pet"]));
    assert!(t.undetermined.is_empty());
}

#[cfg(feature = "shacl")]
#[test]
fn tightening_a_cardinality_reports_the_exact_count_that_would_newly_fail() {
    let store = store_with(PEOPLE, 0);
    store.load_shapes("people", &email_shape(1, 2), T0).unwrap();
    let id = propose(&store, &ProposalKind::Shape, &email_shape(1, 1));

    let plan = plan(&store, id, 40).unwrap();

    assert_eq!(plan.instances_in_scope, 5);
    // alice, bob, carol: newly over maxCount 1. erin already fails minCount,
    // so her violation is debt, not new.
    assert_eq!(plan.newly_failing.instances, 3, "{:#?}", plan.newly_failing);
    assert_eq!(plan.newly_failing.violations, 3);
    let mut focus: Vec<_> = plan
        .newly_failing
        .sample
        .iter()
        .map(|i| i.focus_node.trim_matches(['<', '>']).to_owned())
        .collect();
    focus.sort();
    assert_eq!(focus, ["alice", "bob", "carol"].map(|l| format!("{EX}{l}")));
    assert!(
        plan.newly_failing
            .sample
            .iter()
            .all(|i| i.component.contains("MaxCount"))
    );
    assert_eq!(plan.risk.inputs[0].name, "newly_failing_instances");
    assert_eq!(plan.risk.inputs[0].points, 20);
    assert_eq!(plan.risk.verdict, Verdict::Pass);
}

#[cfg(feature = "shacl")]
#[test]
fn a_plan_writes_nothing() {
    let store = store_with(PEOPLE, 0);
    store.load_shapes("people", &email_shape(1, 2), T0).unwrap();
    let id = propose(&store, &ProposalKind::Shape, &email_shape(1, 1));
    let before = store.list_shapes().unwrap();
    let tx = store.latest_tx_id().unwrap();

    plan(&store, id, 40).unwrap();

    assert_eq!(store.list_shapes().unwrap(), before);
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    let p = store.get_proposal(id).unwrap().unwrap();
    assert_eq!(p.status, ProposalStatus::Pending);
}

#[test]
fn dropped_terms_queries_and_named_graphs_are_reported() {
    let mut store = store_with(PEOPLE, 0);
    let g = store.graph_create("http://example.org/g/crm").unwrap();
    ingest(
        &mut store,
        "<http://example.org/zed> <http://example.org/email> \"z\" .",
        g,
    );
    store.load_shapes("people", &email_shape(0, 9), T0).unwrap();
    for (name, template) in [
        (
            "by_iri",
            "SELECT ?s WHERE { ?s <http://example.org/email> ?o }",
        ),
        (
            "by_prefix",
            "PREFIX ex: <http://example.org/>\nSELECT ?s WHERE { ?s ex:email ?o }",
        ),
        (
            "unrelated",
            "SELECT ?s WHERE { ?s <http://example.org/phone> ?o }",
        ),
    ] {
        let q = StoredQuery {
            name: name.into(),
            description: "d".into(),
            template: template.into(),
            dataset: None,
            params: vec![],
        };
        store.query_load(&q, T0).unwrap();
    }
    // The proposal stops constraining ex:email at all.
    let diff = format!("{PREFIXES}ex:PersonShape a sh:NodeShape ; sh:targetClass ex:Person .");
    let id = propose(&store, &ProposalKind::Shape, &diff);

    let plan = plan(&store, id, 40).unwrap();

    assert_eq!(plan.removed_terms, BTreeSet::from([format!("{EX}email")]));
    assert_eq!(plan.stored_queries, ["by_iri", "by_prefix"]);
    assert_eq!(plan.named_graphs, ["http://example.org/g/crm"]);
    assert!(plan.snapshots.is_empty());
    assert_eq!(plan.newly_failing.instances, 0);
    // removed 5 + queries 10 + graph 2.
    assert_eq!(plan.risk.score, 17);
}

#[test]
fn a_score_over_the_threshold_fails_and_the_gate_needs_a_reason() {
    let store = store_with(PEOPLE, 0);
    let q = StoredQuery {
        name: "emails".into(),
        description: "d".into(),
        template: "SELECT ?s WHERE { ?s <http://example.org/email> ?o }".into(),
        dataset: None,
        params: vec![],
    };
    store.query_load(&q, T0).unwrap();
    let id = propose(&store, &ProposalKind::Property, &email_shape(0, 9));
    let p = plan(&store, id, 0).unwrap();
    assert_eq!(p.risk.score, 5, "{:#?}", p.risk);
    assert_eq!(p.risk.verdict, Verdict::Fail);
    assert_eq!(p.risk.verdict.exit_code(), 1);

    let err = gate(&p, None, None).unwrap_err().to_string();
    assert!(err.contains("remains pending"), "{err}");
    assert!(
        gate(&p, Some("  "), None).is_err(),
        "a blank reason is no reason"
    );
    let note = gate(&p, Some("reviewed with owner"), Some("ok"))
        .unwrap()
        .unwrap();
    assert!(note.starts_with("ok; accepted at risk"), "{note}");
    assert!(note.ends_with("reviewed with owner"), "{note}");

    let passing = plan(&store, id, 100).unwrap();
    assert_eq!(passing.risk.verdict, Verdict::Pass);
    assert_eq!(
        gate(&passing, None, Some("n")).unwrap().as_deref(),
        Some("n")
    );
}

#[test]
fn a_sparql_target_is_undetermined_and_never_passes() {
    let store = store_with(PEOPLE, 0);
    let diff = format!(
        "{PREFIXES}ex:S a sh:NodeShape ; sh:target [ a sh:SPARQLTarget ; \
         sh:select \"SELECT ?this WHERE {{ ?this a ex:Person }}\" ] ."
    );
    let id = propose(&store, &ProposalKind::Shape, &diff);
    let p = plan(&store, id, 100).unwrap();
    assert_eq!(p.risk.verdict, Verdict::Undetermined);
    assert_eq!(p.risk.verdict.exit_code(), 2);
    assert!(!p.undetermined.is_empty());
    assert!(gate(&p, None, None).is_err());
    assert!(render_text(&p).contains("undetermined: "));
}

#[test]
fn a_decided_proposal_cannot_be_planned() {
    let store = store_with(PEOPLE, 0);
    let id = propose(&store, &ProposalKind::Shape, &email_shape(0, 9));
    store.reject_proposal(id, "r", T0, "no").unwrap();
    assert!(plan(&store, id, 40).is_err());
    assert!(plan(&store, 999, 40).is_err());
}

#[test]
fn points_follow_the_documented_table() {
    assert_eq!(points("newly_failing_instances", 0).0, 0);
    assert_eq!(points("newly_failing_instances", 1).0, 20);
    assert_eq!(points("newly_failing_instances", 10).0, 30);
    assert_eq!(points("newly_failing_instances", 999).0, 40);
    assert_eq!(points("newly_failing_instances", 1_000_000).0, 50);
    assert_eq!(points("stored_queries", 100).0, 15);
    assert_eq!(points("named_graphs", 3).0, 6);
    assert_eq!(points("snapshots", 1).0, 5);
    assert_eq!(points("migration_ops", 9).0, 5);
}

#[cfg(feature = "shacl")]
#[test]
fn mcp_accept_refuses_over_threshold_and_records_an_accepted_risk() {
    use crate::mcp::proposal::{tool_accept_proposal, tool_list_proposals};
    use serde_json::json;
    let mut store = store_with(PEOPLE, 0);
    store.load_shapes("people", &email_shape(1, 2), T0).unwrap();
    let id = propose(&store, &ProposalKind::Shape, &email_shape(1, 1));

    let listed = tool_list_proposals(&store, &json!({ "plan": true })).unwrap();
    let plan = &listed["proposals"][0]["plan"];
    assert_eq!(plan["newly_failing"]["instances"], 3, "{plan:#}");
    assert_eq!(plan["risk"]["verdict"], "pass");
    let bare = tool_list_proposals(&store, &json!({})).unwrap();
    assert!(bare["proposals"][0]["plan"].is_null());

    let strict = json!({ "id": id, "decided_by": "r", "risk_threshold": 10 });
    let err = tool_accept_proposal(&mut store, &strict)
        .unwrap_err()
        .to_string();
    assert!(err.contains("risk 20/10"), "{err}");
    let mut no_reason = strict.clone();
    no_reason["accept_risk"] = json!(true);
    assert!(tool_accept_proposal(&mut store, &no_reason).is_err());
    assert_eq!(
        store.get_proposal(id).unwrap().unwrap().status,
        ProposalStatus::Pending
    );

    let mut at_risk = no_reason;
    at_risk["risk_reason"] = json!("owner reviewed the three");
    let out = tool_accept_proposal(&mut store, &at_risk).unwrap();
    assert_eq!(out["status"], "accepted");
    assert_eq!(out["plan"]["risk"]["score"], 20);
    let p = store.get_proposal(id).unwrap().unwrap();
    assert_eq!(
        p.decision_note.as_deref(),
        Some("accepted at risk 20/10 (Fail): owner reviewed the three")
    );
}

#[test]
fn prefixed_names_match_whole_tokens_only() {
    assert!(names_prefixed("SELECT ?s { ?s ex:email ?o }", "ex:email"));
    assert!(names_prefixed("?s ex:email/ex:x ?o", "ex:email"));
    assert!(!names_prefixed("?s ex:emailAddress ?o", "ex:email"));
    assert!(!names_prefixed("?s aex:email ?o", "ex:email"));
}

#[test]
fn accept_of_a_decided_proposal_keeps_the_store_refusal() {
    let mut store = store_with(PEOPLE, 0);
    let id = propose(&store, &ProposalKind::Property, &email_shape(0, 9));
    store.reject_proposal(id, "r", T0, "no").unwrap();
    assert!(plan_for_accept(&store, id, 40).unwrap().is_none());
    let err = crate::mcp::proposal::tool_accept_proposal(
        &mut store,
        &serde_json::json!({ "id": id, "decided_by": "r" }),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("already rejected"), "{err}");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_frozen_graph_using_a_changed_term_is_reported_as_a_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.db").to_string_lossy().to_string();
    let mut store = Store::open(&path).unwrap();
    ingest(&mut store, PEOPLE, 0);
    let live = store.graph_create("http://example.org/g/live").unwrap();
    let cold = store.graph_create("http://example.org/g/2026-07").unwrap();
    let triple = "<http://example.org/zed> <http://example.org/email> \"z\" .";
    ingest(&mut store, triple, live);
    ingest(&mut store, triple, cold);
    let archive = dir.path().join("archive");
    std::fs::create_dir(&archive).unwrap();
    let archive = archive.to_string_lossy().to_string();
    store
        .freeze_graph("http://example.org/g/2026-07", &archive, T0, Some("test"))
        .unwrap();
    let id = propose(&store, &ProposalKind::Property, &email_shape(0, 9));

    let plan = plan(&store, id, 40).unwrap();

    assert_eq!(plan.named_graphs, ["http://example.org/g/live"]);
    assert_eq!(plan.snapshots, ["http://example.org/g/2026-07"]);
}

#[test]
fn a_diff_that_is_not_turtle_is_undetermined_not_an_error() {
    let store = store_with(PEOPLE, 0);
    let id = propose(
        &store,
        &ProposalKind::Ontology,
        r#"{"op": "add", "path": "/x"}"#,
    );
    let p = plan(&store, id, 100).unwrap();
    assert_eq!(p.risk.verdict, Verdict::Undetermined);
    assert!(
        p.undetermined[0].contains("does not parse as Turtle"),
        "{:?}",
        p.undetermined
    );
    // The gate refuses it unless the risk is accepted with a reason.
    assert!(gate(&p, None, None).is_err());
    assert!(gate(&p, Some("JSON patch reviewed by hand"), None).is_ok());
    // A shape proposal with bad Turtle is left to the accept's own refusal.
    let bad = propose(&store, &ProposalKind::Shape, "not turtle {{");
    assert!(plan_for_accept(&store, bad, 40).unwrap().is_none());
}
