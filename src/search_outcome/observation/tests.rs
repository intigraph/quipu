use super::*;

#[test]
fn adapter_input_survives_internal_embedding_and_response_capture() {
    let call = Call::new(Surface::Http, Operation::Search);
    let original = json!({"query":"original", "limit":3});
    call.input(&original);
    call.response(
        &json!({"query":"rewritten", "limit":7, "embedding":[0,1]}),
        &json!({"results":[], "count":0}),
    );
    let event = call.finish(Some(200), None).unwrap();
    assert_eq!(
        event.input_sha256,
        Some(format!(
            "{:x}",
            Sha256::digest(original.to_string().as_bytes())
        ))
    );
    assert_eq!(
        event.query_sha256,
        Some(format!("{:x}", Sha256::digest(b"original")))
    );
    let details = event.details.unwrap();
    assert_eq!(details.query_evidence.as_deref(), Some("original"));
    assert_eq!(details.requested_options.unwrap()["limit"], 3);
}

#[test]
fn forwarding_preserves_original_declarations_and_logs_omit_private_evidence() {
    let call = Call::new(Surface::Mcp, Operation::Query);
    call.declare(&[
        ("client", Some("original-client")),
        ("agent", Some("fixture-agent")),
        ("task", Some("fixture-task")),
        ("authorization", Some("never-retain-authorization")),
    ]);
    call.declare(&[
        ("client", Some("native-mcp")),
        ("session", Some("11111111-2222-4333-8444-555555555555")),
    ]);
    call.scope(|| {
        run(
            Operation::Query,
            &json!({"query":"private-evidence-sentinel"}),
            || Ok(json!({"rows":[],"count":0})),
        )
    })
    .unwrap();
    call.transport_finished(200, None);
    let event = call.finish_mcp(None).unwrap();
    assert_eq!(event.declared_provenance["client"], "original-client");
    assert_eq!(
        event.declared_provenance["session"],
        "11111111-2222-4333-8444-555555555555"
    );
    assert_eq!(event.provenance_knownness, "partial_declared");
    let private = serde_json::to_string(&event).unwrap();
    assert!(private.contains("private-evidence-sentinel"));
    assert!(!private.contains("never-retain-authorization"));
    let public_log = log_record(&event).to_string();
    assert!(!public_log.contains("private-evidence-sentinel"));
    assert!(!public_log.contains("original-client"));
    assert!(!public_log.contains("11111111-2222-4333-8444-555555555555"));
}

#[test]
fn nested_core_calls_share_the_outer_terminal_record() {
    let call = Call::new(Surface::Mcp, Operation::SearchQuery);
    let body = json!({"candidate_count":1,"result":{"rows":[],"count":0}});
    let result = call
        .scope(|| {
            run(Operation::SearchQuery, &json!({"query":"outer"}), || {
                run(Operation::Query, &json!({"query":"inner"}), || {
                    Ok(json!({"result":false}))
                })?;
                Ok(body.clone())
            })
        })
        .unwrap();
    assert_eq!(result, body);
    let event = call.finish(Some(200), None).unwrap();
    assert_eq!(event.surface, Surface::Mcp);
    assert_eq!(event.operation, Operation::SearchQuery);
    assert_eq!(event.summary.outcome, Outcome::Empty);
    assert_eq!(event.summary.candidate_count, Some(1));
    assert_eq!(event.summary.result_count, Some(0));
    assert!(call.finish(Some(200), None).is_none());
    let expected = format!("{:x}", Sha256::digest(b"outer"));
    assert_eq!(event.query_sha256, Some(expected));
}

#[test]
fn transport_failure_overrides_even_a_completed_positive_core_answer() {
    for (status, outcome) in [
        (401, Outcome::Refused),
        (403, Outcome::Refused),
        (408, Outcome::Timeout),
        (504, Outcome::Timeout),
        (499, Outcome::Unknown),
        (500, Outcome::Error),
    ] {
        let call = Call::new(Surface::Http, Operation::Query);
        call.scope(|| {
            run(Operation::Query, &json!({}), || {
                Ok(json!({"rows":[],"count":0}))
            })
        })
        .unwrap();
        let record = call.finish(Some(status), None).unwrap();
        assert_eq!(record.summary, Summary::unavailable(outcome));
        assert!(record.summary.zero_kind.is_none());
    }
}

#[test]
fn late_worker_completion_cannot_rewrite_a_terminal_timeout() {
    let call = Call::new(Surface::Http, Operation::Query);
    let timeout = call.finish(Some(408), None).unwrap();
    let worker = Arc::clone(&call);
    std::thread::spawn(move || {
        worker
            .scope(|| run(Operation::Query, &json!({}), || Ok(json!({"result":true}))))
            .unwrap();
        assert!(worker.finish(Some(200), None).is_none());
    })
    .join()
    .unwrap();
    assert_eq!(timeout.summary.outcome, Outcome::Timeout);
    assert!(timeout.summary.result_count.is_none());
}

#[test]
fn a_reused_worker_restores_its_call_scope_after_unwind() {
    let call = Call::new(Surface::Http, Operation::Search);
    let result = std::panic::catch_unwind(|| call.scope(|| panic!("fixture")));
    assert!(result.is_err());
    assert!(ACTIVE.with(|slot| slot.borrow().is_none()));
}

#[test]
fn unobserved_success_and_refused_request_keep_counts_unknown() {
    let call = Call::new(Surface::Http, Operation::Search);
    let event = call.finish(Some(200), None).unwrap();
    assert_eq!(event.summary, Summary::unavailable(Outcome::Unknown));
    let call = Call::new(Surface::Http, Operation::Search);
    let event = call.finish(Some(403), None).unwrap();
    assert_eq!(event.summary, Summary::unavailable(Outcome::Refused));
    assert!(event.input_sha256.is_none());
}

#[test]
fn private_event_scrubs_credentials_and_omits_arbitrary_parameters() {
    let call = Call::new(Surface::CliLocal, Operation::NamedQuery);
    call.scope(|| {
        run(
            Operation::NamedQuery,
            &json!({"query":"password=private-query-value", "params":{"secret":"private-param-value"}}),
            || Ok(json!({"rows":[],"count":0})),
        )
    })
    .unwrap();
    let event = serde_json::to_string(&call.finish(None, None).unwrap()).unwrap();
    assert!(!event.contains("private-query-value"));
    assert!(!event.contains("private-param-value"));
    assert!(event.contains("quipu-search-outcome/v1"));
}

#[test]
fn actual_routes_have_bounded_operation_labels() {
    for route in [
        "/search",
        "/search_nodes",
        "/search/nodes",
        "/search_facts",
        "/hybrid_search",
        "/search_query",
        "/query",
        "/sparql",
        "/ask",
    ] {
        assert!(for_route(route).is_some(), "{route}");
    }
    for route in ["/entity/private-value", "/search/not-a-route", "/metrics"] {
        assert!(for_route(route).is_none());
    }
}
