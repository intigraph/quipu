//! The advertised choice and real pure resolver must agree; no Store is opened.

use serde_json::{Value, json};

fn convergent_input() -> Value {
    json!({
        "decisions": {
            "schema": crate::fork_merge_decisions::FORK_DECISIONS_SCHEMA,
            "fork": "fixture",
            "fork_tx": 1,
            "rows": [{
                "id": "d1",
                "subject": "<urn:fixture:s>",
                "predicate": "urn:fixture:p",
                "max_count": null,
                "base": ["\"old\""],
                "ours": ["\"same\""],
                "theirs": ["\"same\""],
                "provenance": {"ours": [], "theirs": [], "base_tx": 1},
                "root_slot_hash": "fixture-root",
                "fork_slot_hash": "fixture-fork",
                "kind": "convergent",
                "decision": {"choose": "continue"}
            }]
        },
        "row": "d1",
        "choose": "continue",
        "by": "fixture-reviewer"
    })
}

#[test]
fn advertised_continue_is_accepted_by_the_real_pure_resolver() {
    let definition = super::tool_definitions()
        .into_iter()
        .find(|tool| tool["name"] == "quipu_merge_resolve")
        .unwrap();
    let choices = definition["inputSchema"]["properties"]["choose"]["enum"]
        .as_array()
        .unwrap();
    for choice in [
        "continue", "ours", "theirs", "base", "both", "value", "defer",
    ] {
        assert!(choices.contains(&json!(choice)), "{choice}");
    }
    let output = super::merge_decisions::tool_merge_resolve(&convergent_input()).unwrap();
    assert_eq!(
        output["decisions"]["rows"][0]["decision"]["choose"],
        "continue"
    );
    assert_eq!(
        output["view"]["schema"],
        crate::merge_reconcile::VIEW_SCHEMA
    );
    assert_eq!(
        output["view"]["conflicts"][0]["rule"]["max_count"],
        Value::Null
    );
    assert_eq!(output["view"]["undecided"], 0);
}

#[test]
fn advertised_continue_does_not_admit_a_conflict_with_a_forged_kind() {
    let mut input = convergent_input();
    input["decisions"]["rows"][0]["theirs"] = json!(["\"different\""]);
    input["decisions"]["rows"][0]["max_count"] = json!(1);
    // A caller's kind label is not proof of convergence.
    let error = super::merge_decisions::tool_merge_resolve(&input).unwrap_err();
    assert!(error.to_string().contains("continue"), "{error}");
}
