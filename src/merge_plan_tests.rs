use super::*;

fn policy() -> Policy {
    Policy {
        threshold: 40,
        protected_classes: BTreeSet::from(["urn:Credential".into()]),
        protected_terms: BTreeSet::from(["urn:policy".into()]),
        recall_queries: Some(BTreeMap::new()),
    }
}

#[test]
fn protection_survives_type_removal_and_subclass_cycles() {
    let before = "<urn:a> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:Secret> .\n<urn:a> <urn:value> \"old\" .\n\
            <urn:Secret> <http://www.w3.org/2000/01/rdf-schema#subClassOf> <urn:Credential> .\n\
            <urn:Credential> <http://www.w3.org/2000/01/rdf-schema#subClassOf> <urn:Secret> .";
    let store = Store::open_in_memory().unwrap();
    let plan = plan(&store, before, "<urn:a> <urn:value> \"new\" .", &policy()).unwrap();
    assert!(plan.protected.contains("<urn:a>"));
    assert_eq!(plan.risk.verdict, Verdict::Fail);
}

#[test]
fn reverse_only_dependents_are_included_without_writing_store() {
    let store = Store::open_in_memory().unwrap();
    let id = store.store_id().unwrap();
    let tx = store.latest_tx_id().unwrap();
    let before = "<urn:consumer> <urn:dependsOn> <urn:a> .";
    let after = format!("{before}\n<urn:a> <urn:value> \"new\" .");
    let plan = plan(&store, before, &after, &policy()).unwrap();
    assert!(plan.dependents.contains("<urn:consumer>"));
    assert_eq!(plan.risk.verdict, Verdict::Pass);
    assert_eq!(store.store_id().unwrap(), id);
    assert_eq!(store.latest_tx_id().unwrap(), tx);
}

#[test]
fn missing_protection_catalog_cannot_pass() {
    let store = Store::open_in_memory().unwrap();
    let mut policy = policy();
    policy.protected_classes.clear();
    let plan = plan(&store, "", "", &policy).unwrap();
    assert_eq!(plan.risk.verdict.exit_code(), 2);
    assert!(plan.undetermined.iter().any(|r| r.contains("policy")));
}

#[test]
fn positive_plan_and_mutated_proof_use_the_same_receiver() {
    let store = Store::open_in_memory().unwrap();
    let after = "<urn:a> <urn:value> \"new\" .";
    let report = plan(&store, "", after, &policy()).unwrap();
    assert_eq!(report.risk.verdict, Verdict::Pass);
    let mut evidence = serde_json::to_value(&report).unwrap();
    verify(&store, "", after, &policy(), &evidence).unwrap();
    evidence["risk"]["score"] = serde_json::json!(99);
    assert!(verify(&store, "", after, &policy(), &evidence).is_err());
}

#[test]
fn explicit_missing_recall_catalog_is_unknown() {
    let store = Store::open_in_memory().unwrap();
    let mut policy = policy();
    policy.recall_queries = None;
    let report = plan(&store, "", "<urn:a> <urn:p> \"v\" .", &policy).unwrap();
    assert_eq!(report.risk.verdict, Verdict::Undetermined);
}

#[test]
fn variable_predicate_and_negative_queries_compare_actual_answers() {
    use crate::store::queries::StoredQuery;
    let store = Store::open_in_memory().unwrap();
    for (name, template) in [
        ("variable", "SELECT ?s ?p ?o WHERE { ?s ?p ?o }"),
        (
            "negative",
            "ASK { <urn:a> <urn:p> ?v FILTER NOT EXISTS { <urn:a> <urn:q> ?x } }",
        ),
    ] {
        store
            .query_load(
                &StoredQuery {
                    name: name.into(),
                    description: String::new(),
                    template: template.into(),
                    dataset: None,
                    params: vec![],
                },
                "2000-01-01T00:00:00Z",
            )
            .unwrap();
    }
    let before = "<urn:a> <urn:p> \"v\" .";
    let after = "<urn:a> <urn:p> \"v\" .\n<urn:a> <urn:q> \"v\" .";
    let report = plan(&store, before, after, &policy()).unwrap();
    assert_eq!(report.risk.verdict, Verdict::Pass);
    assert_eq!(
        report
            .changed_queries
            .iter()
            .map(|q| q.name.as_str())
            .collect::<Vec<_>>(),
        ["negative", "variable"]
    );
}

#[test]
fn unsupported_remote_query_never_runs_or_passes() {
    let store = Store::open_in_memory().unwrap();
    let mut policy = policy();
    policy.recall_queries = Some(BTreeMap::from([(
        "remote".into(),
        "SELECT ?s WHERE { SERVICE <http://example.invalid/sparql> { ?s ?p ?o } }".into(),
    )]));
    let report = plan(&store, "", "", &policy).unwrap();
    assert_eq!(report.risk.verdict, Verdict::Undetermined);
    assert!(report.undetermined.iter().any(|s| s.contains("remote")));
}

#[test]
fn protected_predicate_and_receipt_only_effects_are_held() {
    let store = Store::open_in_memory().unwrap();
    let protected = plan(&store, "", "<urn:a> <urn:policy> \"v\" .", &policy()).unwrap();
    assert_eq!(protected.risk.verdict, Verdict::Fail);
    let touched = BTreeSet::from([("<urn:a>".into(), "urn:policy".into())]);
    let receipt = plan_touched(&store, "", "", &policy(), &touched).unwrap();
    assert_eq!(receipt.risk.verdict, Verdict::Fail);
    assert!(receipt.protected.contains("<urn:a>"));
}

#[cfg(feature = "shacl")]
#[test]
fn newly_failing_shapes_exclude_preexisting_debt() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes(
            "risk",
            "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
       <urn:S> a sh:NodeShape ; sh:targetClass <urn:Person> ;\n\
       sh:property [ sh:path <urn:email>; sh:minCount 1 ] .",
            "2000-01-01T00:00:00Z",
        )
        .unwrap();
    let before = "<urn:a> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:Person> .";
    let after = format!(
        "{before}\n<urn:b> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:Person> ."
    );
    let report = plan(&store, before, &after, &policy()).unwrap();
    assert_eq!(report.preexisting_issues, 1);
    assert_eq!(report.newly_failing_issues, 1);
    assert_eq!(report.newly_failing_instances, 1);
}

#[test]
fn threshold_equality_passes_and_one_less_holds() {
    let store = Store::open_in_memory().unwrap();
    let after = "<urn:a> <urn:q> \"v\" .";
    let base = plan(&store, "", after, &policy()).unwrap();
    assert!(base.risk.score > 0);
    let mut policy = policy();
    policy.threshold = base.risk.score;
    assert_eq!(
        plan(&store, "", after, &policy).unwrap().risk.verdict,
        Verdict::Pass
    );
    policy.threshold -= 1;
    assert_eq!(
        plan(&store, "", after, &policy).unwrap().risk.verdict,
        Verdict::Fail
    );
}

#[test]
fn changed_policy_or_head_invalidates_the_original_proof() {
    let mut store = Store::open_in_memory().unwrap();
    let report = plan(&store, "", "", &policy()).unwrap();
    let evidence = serde_json::to_value(&report).unwrap();
    let mut changed = policy();
    changed.protected_terms.insert("urn:extra".into());
    assert!(verify(&store, "", "", &changed, &evidence).is_err());
    store
        .transact(&[], "2000-01-01T00:00:00Z", None, None)
        .unwrap();
    assert!(verify(&store, "", "", &policy(), &evidence).is_err());
}

#[test]
fn blank_node_identity_cannot_be_a_false_pass() {
    let store = Store::open_in_memory().unwrap();
    let report = plan(&store, "", "_:a <urn:p> \"v\" .", &policy()).unwrap();
    assert_eq!(report.risk.verdict, Verdict::Undetermined);
}

#[test]
fn duplicate_answers_are_not_collapsed_to_a_set() {
    use crate::store::queries::StoredQuery;
    let store = Store::open_in_memory().unwrap();
    store
        .query_load(
            &StoredQuery {
                name: "bags".into(),
                description: String::new(),
                template: "SELECT ?v WHERE { ?s <urn:p> ?v }".into(),
                dataset: None,
                params: vec![],
            },
            "2000-01-01T00:00:00Z",
        )
        .unwrap();
    let report = plan(
        &store,
        "<urn:a> <urn:p> \"v\" .",
        "<urn:a> <urn:p> \"v\" .\n<urn:b> <urn:p> \"v\" .",
        &policy(),
    )
    .unwrap();
    assert_eq!(report.changed_queries.len(), 1);
    assert_ne!(
        report.changed_queries[0].before,
        report.changed_queries[0].after
    );
}

#[test]
fn named_graph_context_is_reported_and_cannot_false_pass() {
    let store = Store::open_in_memory().unwrap();
    store.graph_create("urn:named").unwrap();
    let report = plan(&store, "", "", &policy()).unwrap();
    assert!(report.named_graphs.contains(&"urn:named".into()));
    assert_eq!(report.risk.verdict, Verdict::Undetermined);
}

#[cfg(not(feature = "shacl"))]
#[test]
fn compiled_out_shapes_are_unknown() {
    let empty = parse_graph("", "fixture").unwrap();
    assert!(
        scan::issues(
            "@prefix sh: <http://www.w3.org/ns/shacl#> . [] a sh:NodeShape .",
            &empty,
            &empty
        )
        .is_err()
    );
}
