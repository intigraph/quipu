//! Regression checks at the HTTP handler boundary, with isolated stores.
use super::*;
use axum::{Json, extract::State, http::HeaderMap};
use serde_json::json;
use std::sync::Arc;

fn store() -> super::super::SharedStore {
    Arc::new(super::super::StoreHandle::writer_only(
        quipu::Store::open_in_memory().unwrap(),
    ))
}

#[tokio::test]
async fn graph_metadata_selector_at_http_handler_preserves_envelope_and_errors() {
    use axum::extract::Query;
    use axum::response::IntoResponse;
    let shared = store();
    shared
        .lock()
        .graph_create_recorded("urn:fixture:point", "2026-10-20T00:00:00Z", Some("fixture"))
        .unwrap();
    shared.lock().graph_create("urn:fixture:other").unwrap();
    let params =
        std::collections::HashMap::from([("graph".to_string(), "urn:fixture:point".to_string())]);
    let Json(result) =
        super::super::tools::graphs_list(State(shared.clone()), Query(params.clone()))
            .await
            .unwrap();
    assert_eq!(result["count"], 1);
    assert_eq!(result["graphs"][0]["creation"]["actor"], "fixture");
    assert!(result["graphs"][0].get("materialization").is_none());
    let mut combined = params;
    combined.insert("lifecycle".into(), "frozen".into());
    let error = super::super::tools::graphs_list(State(shared.clone()), Query(combined))
        .await
        .unwrap_err();
    assert_eq!(
        error.into_response().status(),
        axum::http::StatusCode::BAD_REQUEST
    );
    let Json(missing) = super::super::tools::graphs_list(
        State(shared.clone()),
        Query(std::collections::HashMap::from([(
            "graph".into(),
            "urn:fixture:absent".into(),
        )])),
    )
    .await
    .unwrap();
    assert_eq!(missing["count"], 0);
    assert_eq!(shared.read().lookup("urn:fixture:absent").unwrap(), None);
    let Json(list) =
        super::super::tools::graphs_list(State(shared), Query(std::collections::HashMap::new()))
            .await
            .unwrap();
    assert_eq!(list["count"], 3); // ROOT and two named graphs; metadata excluded.
}

#[tokio::test]
async fn root_mutations_reject_graph_with_http_400_and_preserve_both_graphs() {
    use axum::response::IntoResponse;
    for operation in ["set", "retract"] {
        for graph in [json!("https://example.org/named"), json!(null)] {
            let shared = store();
            {
                let mut writer = shared.lock();
                quipu::tool_knot(
                    &mut writer,
                    &json!({"turtle": "<https://example.org/item> <https://example.org/value> \"root\" ."}),
                )
                .unwrap();
                let named = writer
                    .overlay_create("https://example.org/named", 0)
                    .unwrap();
                let entity = writer.lookup("https://example.org/item").unwrap().unwrap();
                let predicate = writer.lookup("https://example.org/value").unwrap().unwrap();
                writer
                    .overlay_write(
                        named,
                        quipu::types::Op::Assert,
                        entity,
                        predicate,
                        quipu::types::Value::Str("named".into()),
                        "2026-01-01T00:00:00Z",
                    )
                    .unwrap();
            }
            let query = json!({"query": "SELECT ?g ?v WHERE { { <https://example.org/item> <https://example.org/value> ?v } UNION { GRAPH ?g { <https://example.org/item> <https://example.org/value> ?v } } }"});
            let before = quipu::tool_query(&shared.read(), &query).unwrap();
            assert_eq!(before["count"], 2);
            let transactions = shared.read().list_transactions().unwrap().len();
            let input = json!({"entity":"https://example.org/item", "predicate":"https://example.org/value",
                "value":"replacement", "graph":graph, "allow_orphan":true});
            let result = if operation == "set" {
                super::super::tools::set_predicate(
                    State(shared.clone()),
                    axum::http::HeaderMap::new(),
                    None,
                    Json(input),
                )
                .await
            } else {
                super::super::tools::retract(
                    State(shared.clone()),
                    axum::http::HeaderMap::new(),
                    None,
                    Json(input),
                )
                .await
            };
            let response = result.unwrap_err().into_response();
            assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
            assert_eq!(quipu::tool_query(&shared.read(), &query).unwrap(), before);
            assert_eq!(
                shared.read().list_transactions().unwrap().len(),
                transactions
            );
        }
    }
}

#[test]
fn schemas_accept_declared_fields_and_report_sorted_unknowns() {
    for definition in quipu::tool_definitions() {
        let name = definition["name"].as_str().unwrap();
        let input = definition["inputSchema"]["properties"].clone();
        assert!(ignored(name, &input).is_empty(), "{name}");
        assert_eq!(annotate(name, &input, json!({})), json!({}));
        let mut extra = input;
        extra["zzz_unknown"] = json!(true);
        extra["aaa_unknown"] = json!(null);
        assert_eq!(
            annotate(name, &extra, json!({"result": 1})),
            json!({"result": 1, "ignored_fields": ["aaa_unknown", "zzz_unknown"]}),
            "{name}"
        );
    }
}

#[test]
fn every_macro_handler_resolves_a_schema() {
    let source = syn::parse_file(include_str!("tools.rs")).unwrap();
    let mut count = 0;
    for item in source.items {
        let syn::Item::Macro(item) = item else {
            continue;
        };
        if !["ro_handler", "rw_handler", "embed_handler"]
            .iter()
            .any(|name| item.mac.path.is_ident(name))
        {
            continue;
        }
        let tokens = item.mac.tokens.to_string();
        let tool = tokens
            .split_once(',')
            .unwrap()
            .1
            .trim()
            .trim_end_matches(',');
        if tool.contains("tool_load_ontology") && !cfg!(feature = "owl") {
            continue;
        }
        assert_eq!(ignored(tool, &json!({"__unknown": 1})), vec!["__unknown"]);
        count += 1;
    }
    assert!(count > 30, "macro coverage control: {count}");
}

#[test]
fn graphiti_adapter_reports_verbose_but_accepts_its_actual_fields() {
    let input = json!({"query":"sample", "group_ids":[], "max_results":2,
        "entity_type_filter":"https://example.org/Type", "verbose":true});
    assert_eq!(
        ignored("quipu :: mcp :: graphiti :: tool_search_nodes", &input),
        vec!["verbose"]
    );
    assert!(ignored("quipu::tool_search_nodes", &input).is_empty());
}

#[tokio::test]
async fn knot_unknown_dry_run_reports_warning_and_still_writes() {
    let shared = store();
    let response = super::super::publication::knot(
        State(shared.clone()),
        Json(json!({
            "turtle": "<https://example.org/item> <https://example.org/value> \"written\" .",
            "dry_run": true
        })),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(response["ignored_fields"], json!(["dry_run"]));
    let result = quipu::tool_query(
        &shared.read(),
        &json!({
            "query":"SELECT ?v WHERE { <https://example.org/item> <https://example.org/value> ?v }"
        }),
    )
    .unwrap();
    assert_eq!(
        result["count"], 1,
        "reporting must preserve the existing write"
    );
    let response = super::super::publication::knot(
        State(shared),
        Json(json!({
            "turtle": "<https://example.org/clean> <https://example.org/value> \"written\" ."
        })),
    )
    .await
    .unwrap()
    .0;
    assert!(response.get("ignored_fields").is_none());
}

#[tokio::test]
async fn handler_errors_are_preserved() {
    assert!(
        super::super::publication::knot(
            State(store()),
            Json(json!({
                "dry_run": true
            }))
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn macro_read_and_write_handlers_report_fields() {
    let shared = store();
    let response = super::super::tools::search_nodes(
        State(shared.clone()),
        Json(json!({
            "query":"nothing", "typo":true
        })),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(response["ignored_fields"], json!(["typo"]));
    let response = super::super::tools::datasets(
        State(shared),
        axum::http::HeaderMap::new(),
        None,
        Json(json!({
            "action":"list", "typo":true
        })),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(response["ignored_fields"], json!(["typo"]));
}

#[tokio::test]
async fn query_json_reports_fields_and_standard_json_preserves_format() {
    let shared = store();
    let input = json!({"query":"SELECT ?s WHERE { ?s ?p ?o }", "dry_run":true});
    let response = super::super::query_endpoint::query(
        State(shared.clone()),
        HeaderMap::new(),
        Json(input.clone()),
    )
    .await
    .unwrap();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: JsonValue = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["ignored_fields"], json!(["dry_run"]));
    let mut headers = HeaderMap::new();
    headers.insert("accept", "application/sparql-results+json".parse().unwrap());
    let response = super::super::query_endpoint::query(State(shared), headers, Json(input))
        .await
        .unwrap();
    assert_eq!(
        response.headers()["x-quipu-ignored-fields"],
        "[\"dry_run\"]"
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: JsonValue = serde_json::from_slice(&body).unwrap();
    assert!(value.get("ignored_fields").is_none());
    assert!(value.get("head").is_some());
    assert!(value.get("results").is_some());
}

async fn export_status(input: JsonValue) -> (axum::http::StatusCode, axum::body::Bytes) {
    use axum::response::IntoResponse;
    let response = match super::super::publication::export(State(store()), Json(input)).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, body)
}

#[tokio::test]
async fn export_refuses_unrecognised_scope_fields_instead_of_exporting_root() {
    // Measured live: both bodies returned 200 with the WHOLE store.
    for input in [
        json!({"scope_kind":"graph", "scope_value":"urn:example:graph"}),
        json!({"scope":{"kind":"graph", "value":"urn:example:graph"}}),
        json!({"format":"ntriples", "dry_run":true}),
        json!([{"graph":"urn:example:graph"}]),
    ] {
        let (status, body) = export_status(input.clone()).await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "{input}");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("nothing was exported"), "{input}: {text}");
    }
}

#[tokio::test]
async fn export_with_no_scope_is_still_the_documented_root_export() {
    let (status, body) = export_status(json!({"format":"ntriples"})).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let expected = quipu::export_rdf_subset(
        &quipu::Store::open_in_memory().unwrap(),
        oxrdfio::RdfFormat::NTriples,
        None,
    )
    .unwrap()
    .0;
    assert_eq!(body.as_ref(), expected.as_slice());
}

#[test]
fn headers_encode_arbitrary_field_names_and_omit_clean_requests() {
    let mut headers = HeaderMap::new();
    query_header(
        &json!({"query":"ASK {}", "federated":false, "_sparql_protocol":true}),
        &mut headers,
    );
    assert!(headers.get("x-quipu-ignored-fields").is_none());
    let input = json!({"é\n🦀": true});
    header("quipu_knot", &input, &mut headers);
    let names: Vec<String> =
        serde_json::from_str(headers["x-quipu-ignored-fields"].to_str().unwrap()).unwrap();
    assert_eq!(names, vec!["é\n🦀"]);
}

#[tokio::test]
async fn shacl_refusal_body_names_each_refused_focus() {
    use axum::response::IntoResponse;
    let messages: Vec<String> = (0..60)
        .map(|i| format!("post-state sh:MaxCountConstraintComponent on urn:focus:{i}: MaxCount(1)"))
        .collect();
    let error = quipu::Error::ValidationFailed {
        violations: messages.len(),
        messages,
    };
    let response = super::super::base::AppError::from(error).into_response();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"], "SHACL validation failed: 60 violation(s)");
    let shown = body["violations"].as_array().unwrap();
    assert_eq!(shown.len(), 50);
    assert!(shown[0].as_str().unwrap().contains("urn:focus:0"));
    assert_eq!(body["violations_truncated"], true);
}
