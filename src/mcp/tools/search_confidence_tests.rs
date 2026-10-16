use super::*;

fn candidate(text: &str, similarity: f64) -> Value {
    json!({"entity":"urn:fixture","text":text,"similarity":similarity,"score":similarity,
        "source":"knowledge","valid_from":"2026-01-01","valid_to":null})
}

fn response(query: &str, rows: &[Value]) -> Value {
    let mut response = json!({"count":rows.len(),"results":rows});
    Policy::parse(&json!({}))
        .unwrap()
        .annotate(&json!({"query":query}), &mut response)
        .unwrap();
    response
}

#[test]
fn three_arms_separate_domain_vocabulary_from_subject_evidence() {
    // Measured score shapes, deliberately small controlled text fixtures.
    // These are not claims about today's whole live graph. Synthetic subjects
    // are constructed so indexing this source cannot manufacture their evidence.
    for (query, text, similarity, reason) in [
        (
            concat!("quaffle", "plex service restart loop journal evidence"),
            "Service restart journal evidence",
            0.566286257,
            "subject_not_covered",
        ),
        (
            concat!("Cassan", "dra nodetool status replication factor"),
            "Quipu RDF performance benchmark",
            0.393081124,
            "no_result_above_floor",
        ),
        (
            "quipu read pool FIFO admission stall",
            "Quipu read pool contention proof",
            0.604687809,
            "sufficient_context",
        ),
    ] {
        let row = candidate(text, similarity);
        let out = response(query, std::slice::from_ref(&row));
        let accepted = reason == "sufficient_context";
        assert_eq!(out["insufficient_context"], !accepted, "{query}");
        assert_eq!(out["context"]["reason"], reason);
        assert_eq!(
            out["results"],
            json!([row]),
            "diagnostic ranking must remain intact"
        );
        assert_eq!(
            out["context_results"].as_array().unwrap().len(),
            usize::from(accepted)
        );
        assert_eq!(
            out["context"]["outcome"],
            if accepted { "CONTEXT" } else { "NO CONTEXT" }
        );
    }
}

#[test]
fn identifiers_are_exact_case_insensitive_and_not_substrings() {
    for id in [
        "aegis-ab12",
        "worker.service",
        "node.example",
        "deadbeef1234",
        "urn:thing/x",
    ] {
        let query = format!("what is {id}");
        let exact = response(
            &query,
            &[candidate(&format!("({})", id.to_uppercase()), 0.8)],
        );
        assert_eq!(exact["insufficient_context"], false, "{id}");
        for wrong in [
            format!("prefix{id}"),
            format!("{id}-other"),
            format!("{id}/child"),
        ] {
            let out = response(&query, &[candidate(&wrong, 0.9)]);
            assert_eq!(out["insufficient_context"], true, "{wrong}");
        }
    }
}

#[test]
fn floor_is_inclusive_and_fused_scores_cannot_stand_in_for_cosine() {
    assert_eq!(
        response("memory", &[candidate("memory", 0.52)])["insufficient_context"],
        false
    );
    assert_eq!(
        response("memory", &[candidate("memory", 0.51999)])["insufficient_context"],
        true
    );
    for row in [
        json!({"text":"memory","score":100}),
        json!({"text":"memory","score":100,"similarity":null}),
        json!({"text":"memory","score":100,"similarity":1.1}),
    ] {
        assert_eq!(
            response("memory", &[row])["context"]["reason"],
            "similarity_unavailable"
        );
    }
    let mut low = candidate("memory", 0.1);
    low["score"] = json!(100);
    assert_eq!(response("memory", &[low])["insufficient_context"], true);
}

#[test]
fn coverage_and_top_hit_must_both_pass_and_provenance_survives() {
    let weak = candidate("memory", 0.9);
    let strong = candidate("memory scheduler allocation budget", 0.8);
    let out = response(
        "memory scheduler allocation budget",
        &[weak, strong.clone()],
    );
    assert_eq!(out["context"]["reason"], "insufficient_term_coverage");
    assert_eq!(out["context_results"], json!([]));
    let out = response(
        "memory scheduler allocation budget",
        &[strong.clone(), candidate("other", 0.7)],
    );
    assert_eq!(out["context_results"], json!([strong]));
    assert_eq!(out["context_results"][0]["valid_from"], "2026-01-01");
    assert_eq!(out["context_results"][0]["source"], "knowledge");
}

#[test]
fn missing_query_empty_candidates_and_invalid_floors_are_explicit() {
    assert_eq!(
        response("", &[candidate("memory", 0.9)])["context"]["reason"],
        "query_terms_unavailable"
    );
    assert_eq!(response("what is the", &[])["insufficient_context"], true);
    assert_eq!(
        response("memory", &[])["context"]["reason"],
        "no_candidates"
    );
    assert_eq!(
        response(&"é".repeat(4097), &[])["context"]["reason"],
        "query_too_long"
    );
    for floor in [json!(-0.1), json!(1.1), json!("0.52"), Value::Null] {
        assert!(Policy::parse(&json!({"context_similarity_floor":floor})).is_err());
    }
}

#[test]
fn actual_search_carries_context_contract_without_rewriting_ranking() {
    use crate::vector::KnowledgeVectorStore;
    let store = crate::Store::open_in_memory().unwrap();
    let id = store.intern("urn:memory").unwrap();
    store
        .embed_entity(id, "memory scheduler allocation", &[1.0, 0.0], "2026-01-01")
        .unwrap();
    let query = json!({"query":"memory scheduler allocation","embedding":[1.0,0.0]});
    let out = crate::tool_search(&store, &query).unwrap();
    assert_eq!(out["context"]["contract"], "quipu-context/v1");
    assert_eq!(out["insufficient_context"], false, "{out}");
    assert_eq!(out["context_results"], out["results"]);
    let missing = crate::tool_search(
        &store,
        &json!({"query":concat!("quaffle", "plex scheduler allocation"),"embedding":[1.0,0.0]}),
    )
    .unwrap();
    assert_eq!(missing["insufficient_context"], true);
    assert_eq!(missing["context_results"], json!([]));
    assert_eq!(missing["results"].as_array().unwrap().len(), 1);
}

#[test]
fn malformed_backend_response_is_an_error_not_zero_context() {
    let policy = Policy::parse(&json!({})).unwrap();
    for mut body in [
        json!({"error":"unavailable"}),
        json!({"results":null}),
        json!([]),
    ] {
        assert!(
            policy
                .annotate(&json!({"query":"memory"}), &mut body)
                .is_err()
        );
    }
}

#[test]
fn six_historical_score_arrays_project_to_the_provisional_floor() {
    // Dated reported cosine arrays, not reconstructed historical response text.
    // Controlled matching text isolates the floor from subject/coverage tests.
    for (scores, expected) in [
        ([0.533, 0.522, 0.477], true),
        ([0.574, 0.572, 0.559], true),
        ([0.482, 0.454, 0.452], false),
        ([0.392, 0.383, 0.370], false),
        ([0.484, 0.482, 0.473], false),
        ([0.510, 0.495, 0.492], false),
    ] {
        let rows: Vec<_> = scores
            .iter()
            .map(|score| candidate("reference subject", *score))
            .collect();
        let out = response("reference subject", &rows);
        assert_eq!(out["insufficient_context"], !expected);
        assert_eq!(out["results"], json!(rows));
        if !expected {
            assert_eq!(out["context"]["reason"], "no_result_above_floor");
        }
    }
}
