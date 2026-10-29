use super::*;
use serde_json::json;

fn response(candidates: usize, admitted: usize) -> Value {
    let rows: Vec<_> = (0..candidates).map(|i| json!({"entity":i})).collect();
    json!({
        "results":rows, "count":candidates,
        "context_results":rows[..admitted].to_vec(),
        "insufficient_context":admitted == 0,
        "context":{
            "contract":"quipu-context/v1",
            "outcome":if admitted == 0 {"NO CONTEXT"} else {"CONTEXT"},
            "reason":if candidates == 0 {"no_candidates"} else if admitted == 0 {"identifier_not_covered"} else {"sufficient_context"},
            "candidate_count":candidates,"admitted_count":admitted
        }
    })
}

#[test]
fn candidate_and_admission_counts_are_separate_stages() {
    let hits = classify(Operation::Search, &response(3, 1));
    assert_eq!(hits.outcome, Outcome::Hits);
    assert_eq!(hits.candidate_count, Some(3));
    assert_eq!(hits.admitted_count, Some(1));
    assert_eq!(hits.zero_kind, None);

    let rejected = classify(Operation::Search, &response(3, 0));
    assert_eq!(rejected.outcome, Outcome::NoContext);
    assert_eq!(rejected.zero_kind, Some(ZeroKind::AllRejected));
    assert_eq!(rejected.candidate_count, Some(3));
    assert_eq!(rejected.admitted_count, Some(0));

    let empty = classify(Operation::Search, &response(0, 0));
    assert_eq!(empty.outcome, Outcome::Empty);
    assert_eq!(empty.zero_kind, Some(ZeroKind::NoCandidates));
}

#[test]
fn damaged_contracts_cannot_manufacture_a_zero_or_a_hit() {
    for (candidates, admitted) in [(0, 0), (3, 0), (3, 1)] {
        let valid = response(candidates, admitted);
        for key in ["context", "context_results", "insufficient_context"] {
            let mut broken = valid.clone();
            broken.as_object_mut().unwrap().remove(key);
            let got = classify(Operation::Search, &broken);
            assert_eq!(got.outcome, Outcome::Unknown, "missing {key}");
            assert_eq!(got.zero_kind, None);
            assert_eq!(got.admitted_count, None);
        }
        for key in [
            "contract",
            "outcome",
            "candidate_count",
            "admitted_count",
            "reason",
        ] {
            for replacement in [Value::Null, json!({}), json!(-1), json!("unexpected")] {
                let mut broken = valid.clone();
                broken["context"][key] = replacement;
                // Reasons are evidence vocabulary owned by the admission policy:
                // a new nonempty reason is allowed, not guessed into another one.
                if key == "reason" && broken["context"][key].is_string() {
                    continue;
                }
                let got = classify(Operation::Search, &broken);
                assert_eq!(got.outcome, Outcome::Unknown, "{key}: {broken}");
                assert_eq!(got.zero_kind, None);
                assert_eq!(got.admitted_count, None);
            }
        }
    }
}

#[test]
fn inconsistent_arrays_counts_and_outcomes_remain_unknown() {
    let mut cases = Vec::new();
    let mut add = |key: &str, value| {
        let mut broken = response(3, 1);
        broken[key] = value;
        cases.push(broken);
    };
    add("count", json!(0));
    add("results", json!([null]));
    add("context_results", json!([{"entity":"not-a-candidate"}]));
    add("context_results", json!([]));
    add("insufficient_context", json!(true));
    let mut contradict = response(3, 0);
    contradict["context"]["outcome"] = json!("CONTEXT");
    cases.push(contradict);
    for broken in cases {
        let got = classify(Operation::Search, &broken);
        assert_eq!(got.outcome, Outcome::Unknown, "{broken}");
        assert_eq!(got.zero_kind, None);
    }
}

#[test]
fn successful_error_bodies_and_missing_fields_are_not_empty() {
    for operation in [
        Operation::Search,
        Operation::HybridSearch,
        Operation::SearchNodes,
        Operation::SearchFacts,
        Operation::SearchQuery,
        Operation::Query,
        Operation::NamedQuery,
    ] {
        for body in [
            Value::Null,
            json!([]),
            json!({}),
            json!({"count":0}),
            json!({"error":"rate limited","rows":[],"count":0}),
            json!({"isError":true,"rows":[]}),
            json!({"rows":null}),
        ] {
            let out = classify(operation, &body);
            assert_eq!(out.outcome, Outcome::Unknown, "{operation:?}: {body}");
            assert_eq!(out.zero_kind, None);
        }
    }
}

#[test]
fn ask_false_is_an_answer_and_expected_empty_relations_are_valid() {
    for operation in [Operation::Query, Operation::NamedQuery] {
        for answer in [true, false] {
            let out = classify(operation, &json!({"result":answer}));
            assert_eq!(out.outcome, Outcome::Hits);
            assert_eq!(out.result_count, Some(1));
            assert_eq!(out.zero_kind, None);
        }
        for key in ["rows", "triples"] {
            let out = classify(operation, &json!({key:[],"count":0}));
            assert_eq!(out.outcome, Outcome::Empty);
            assert_eq!(out.zero_kind, Some(ZeroKind::EmptyRows));
        }
        for malformed in [
            json!({"rows":[null]}),
            json!({"rows":[],"count":1}),
            json!({"result":false,"rows":[]}),
            json!({"rows":[],"triples":[]}),
        ] {
            assert_eq!(classify(operation, &malformed).outcome, Outcome::Unknown);
        }
    }
}

#[test]
fn search_rooted_queries_classify_the_result_not_the_seed_count() {
    let out = classify(
        Operation::SearchQuery,
        &json!({
            "candidate_count":5,"seeds":[{"entity":"x"}],
            "result":{"rows":[],"count":0}
        }),
    );
    assert_eq!(out.outcome, Outcome::Empty);
    assert_eq!(out.candidate_count, Some(5));
    assert_eq!(out.result_count, Some(0));
    assert_eq!(out.zero_kind, Some(ZeroKind::EmptyRows));
    assert_eq!(
        classify(Operation::SearchQuery, &json!({"seed_count":0})).outcome,
        Outcome::Unknown
    );
}

#[test]
fn compatibility_responses_never_invent_admitted_context() {
    for (operation, key) in [
        (Operation::HybridSearch, "results"),
        (Operation::SearchNodes, "nodes"),
        (Operation::SearchFacts, "facts"),
    ] {
        let out = classify(operation, &json!({key:[{"id":1}],"count":1}));
        assert_eq!(out.outcome, Outcome::Hits);
        assert_eq!(out.candidate_count, Some(1));
        assert_eq!(out.admitted_count, None);
        let empty = classify(operation, &json!({key:[],"count":0}));
        assert_eq!(empty.zero_kind, Some(ZeroKind::NoCandidates));
    }
}

#[test]
fn execution_failures_have_nullable_counts_and_no_zero_kind() {
    for outcome in [
        Outcome::Error,
        Outcome::Timeout,
        Outcome::Refused,
        Outcome::Unknown,
    ] {
        let record = serde_json::to_value(Summary::unavailable(outcome)).unwrap();
        for field in [
            "candidate_count",
            "admitted_count",
            "result_count",
            "zero_kind",
        ] {
            assert!(record[field].is_null(), "{record}");
        }
    }
}
