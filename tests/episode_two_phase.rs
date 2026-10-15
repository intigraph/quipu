//! Two-phase `/episode` (aegis-xi0): resolve on a reader, revalidate and commit
//! under the writer. The commit must answer exactly as the single-phase path
//! does, and must see what committed between the two phases.
use quipu::{
    Store,
    mcp::tools::{tool_episode, tool_episode_commit, tool_episode_prepare},
    time::{Deadline, set_request_deadline},
};
use serde_json::{Value, json};

fn store(threshold: f64) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let shapes = format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:document-shape> a sh:NodeShape ; sh:targetClass <{}Document> .",
        store.base_ns()
    );
    quipu::mcp::tools::tool_shapes(
        &store,
        &json!({"action":"load", "name":"documents", "turtle":shapes}),
    )
    .unwrap();
    let resolution = store.resolution_config_mut();
    resolution.enabled = true;
    resolution.threshold = threshold;
    store
}

fn episode(name: &str, nodes: &[&str]) -> Value {
    let nodes: Vec<_> = nodes
        .iter()
        .map(|n| json!({"name": n, "type": "Document", "description": "two-phase control"}))
        .collect();
    json!({"name": name, "nodes": nodes, "edges": []})
}

fn two_phase(store: &mut Store, input: &Value) -> Value {
    let prepared = tool_episode_prepare(store, input)
        .unwrap()
        .expect("non-strict resolution prepares off the writer");
    tool_episode_commit(store, prepared).unwrap()
}

fn hinted(output: &Value, node: &str, candidate: &str) -> bool {
    output["resolution_hints"].to_string().contains(node)
        && output["resolution_hints"].to_string().contains(candidate)
}

/// Seeded with an exact match, a Jaro-Winkler match and a pair whose score
/// EQUALS the threshold (0.88, the #538 review case), both paths must answer
/// byte-identically and write the same facts.
#[test]
fn two_phase_answers_exactly_as_single_phase() {
    let seed = episode(
        "seed",
        &["document-alpha", "aaaaaaaaaa", "unrelated-zeta"],
    );
    let write = episode(
        "write",
        &["document-alpha", "document-alpah", "aaaaaaabbb", "brand-new"],
    );
    let mut single = store(0.88);
    let mut double = store(0.88);
    tool_episode(&mut single, &seed).unwrap();
    tool_episode(&mut double, &seed).unwrap();

    let expected = tool_episode(&mut single, &write).unwrap();
    let actual = two_phase(&mut double, &write);

    assert_eq!(actual, expected);
    assert!(
        hinted(&actual, "aaaaaaabbb", "aaaaaaaaaa"),
        "threshold-equal pair dropped: {actual}"
    );
    assert_eq!(
        single.current_facts().unwrap().len(),
        double.current_facts().unwrap().len()
    );
}

/// A near-duplicate committed after the read must still be hinted: the commit
/// re-checks the labels asserted since the reader's generation.
#[test]
fn near_duplicate_committed_between_phases_is_hinted() {
    let mut store = store(0.85);
    tool_episode(&mut store, &episode("seed", &["unrelated-zeta"])).unwrap();
    let write = episode("write", &["document-gamma"]);
    let prepared = tool_episode_prepare(&store, &write).unwrap().unwrap();

    tool_episode(&mut store, &episode("between", &["document-gamma2"])).unwrap();
    let output = tool_episode_commit(&mut store, prepared).unwrap();

    assert_eq!(output["outcome"], "created");
    assert!(
        hinted(&output, "document-gamma", "document-gamma2"),
        "stale resolution committed without the new near-duplicate: {output}"
    );
}

/// An excusal committed between the phases removes the hint it excuses.
#[test]
fn distinct_from_committed_between_phases_is_honoured() {
    let mut store = store(0.85);
    tool_episode(&mut store, &episode("seed", &["document-delta-x"])).unwrap();
    let write = episode("write", &["document-delta-y"]);
    let prepared = tool_episode_prepare(&store, &write).unwrap().unwrap();

    let x = format!("{}document-delta-x", store.base_ns());
    let excuse = json!({"name": "excuse", "nodes": [{"name": "document-delta-y",
        "type": "Document", "distinct_from": [x]}], "edges": []});
    tool_episode(&mut store, &excuse).unwrap();
    let output = tool_episode_commit(&mut store, prepared).unwrap();

    assert!(
        !hinted(&output, "document-delta-y", "document-delta-x"),
        "an excused pairing was reported: {output}"
    );
}

/// Strict resolution must see the writer's latest state, so it never prepares.
#[test]
fn strict_or_disabled_resolution_stays_single_phase() {
    let mut strict = store(0.85);
    strict.resolution_config_mut().strict_mode = true;
    assert!(
        tool_episode_prepare(&strict, &episode("w", &["a"]))
            .unwrap()
            .is_none()
    );
    let mut off = store(0.85);
    off.resolution_config_mut().enabled = false;
    assert!(
        tool_episode_prepare(&off, &episode("w", &["a"]))
            .unwrap()
            .is_none()
    );
}

/// Turning strict on between the phases cannot be bypassed by a matching
/// generation: the commit re-resolves under the writer and refuses.
#[test]
fn strict_configured_at_commit_refuses_a_prepared_duplicate() {
    let mut store = store(0.85);
    tool_episode(&mut store, &episode("seed", &["document-epsilon"])).unwrap();
    let prepared = tool_episode_prepare(&store, &episode("w", &["document-epsilon"]))
        .unwrap()
        .unwrap();
    store.resolution_config_mut().strict_mode = true;
    let before = store.current_facts().unwrap();
    assert!(tool_episode_commit(&mut store, prepared).is_err());
    assert_eq!(store.current_facts().unwrap(), before);
}

/// An expired budget refuses in phase 0 with the request's own budget, before
/// anything is written; the next write still succeeds.
#[test]
fn expired_budget_refuses_in_phase_zero_without_writing() {
    let mut store = store(0.85);
    let names: Vec<String> = (0..57).map(|i| format!("document-{i}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let before = store.current_facts().unwrap();
    let guard = set_request_deadline(Some(Deadline::after_millis(0)));
    std::thread::sleep(std::time::Duration::from_millis(2));
    let error = tool_episode_prepare(&store, &episode("w", &refs)).unwrap_err();
    drop(guard);
    assert!(
        matches!(error, quipu::Error::RequestTimeout { .. }),
        "{error:?}"
    );
    assert!(!error.to_string().contains("exceeded 0ms"));
    assert_eq!(store.current_facts().unwrap(), before);
    assert_eq!(two_phase(&mut store, &episode("next", &["one"]))["outcome"], "created");
}

/// aegis-8kn: hundreds of nodes with resolution ON commit within budget.
#[test]
fn hundreds_of_resolved_nodes_commit_within_budget() {
    for count in [50, 200] {
        let mut store = store(0.85);
        let names: Vec<String> = (0..count).map(|i| format!("document-{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let before = store.current_facts().unwrap().len();
        let guard = set_request_deadline(Some(Deadline::after_millis(5000)));
        let output = two_phase(&mut store, &episode("batch", &refs));
        drop(guard);
        assert_eq!(output["outcome"], "created");
        assert!(store.current_facts().unwrap().len() - before >= count * 5);
    }
}
