use super::*;
use axum::routing::{get, post};
use serde_json::json;

async fn echo(request: axum::extract::Request) -> axum::Json<Value> {
    let (parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, 1_000_000).await.unwrap();
    axum::Json(
        json!({"method":parts.method.as_str(),"uri":parts.uri.to_string(),
        "type":parts.headers["content-type"].to_str().unwrap(),
        "body":String::from_utf8(bytes.to_vec()).unwrap(),
        "authorization":parts.headers.get("authorization").map(|v|v.to_str().unwrap()),
        "task":parts.headers.get("x-quipu-task").map(|v|v.to_str().unwrap())}),
    )
}
fn payload(result: CallToolResult) -> Value {
    let content = result.content.into_iter().next().unwrap();
    serde_json::from_str(&content.as_text().unwrap().text).unwrap()
}

#[tokio::test]
async fn numeric_get_filters_and_fragment_iris_reach_the_existing_route() {
    let server = McpServer::new(
        Router::new()
            .route("/events", get(echo))
            .route("/entity", get(echo)),
    );
    let result = payload(
        server
            .dispatch(
                "quipu_events",
                json!({"since":42,"limit":3,"consumer":"a&b"}),
                &HeaderMap::new(),
            )
            .await
            .unwrap(),
    );
    let uri: axum::http::Uri = result["uri"].as_str().unwrap().parse().unwrap();
    let fields: std::collections::BTreeMap<_, _> =
        url::form_urlencoded::parse(uri.query().unwrap().as_bytes())
            .into_owned()
            .collect();
    assert_eq!(fields["since"], "42");
    assert_eq!(fields["limit"], "3");
    assert_eq!(fields["consumer"], "a&b");
    let result = payload(
        server
            .dispatch(
                "quipu_entity",
                json!({"iri":"https://example.org/a#b?c=d&z","expanded":true}),
                &HeaderMap::new(),
            )
            .await
            .unwrap(),
    );
    let uri: axum::http::Uri = result["uri"].as_str().unwrap().parse().unwrap();
    let fields: std::collections::BTreeMap<_, _> =
        url::form_urlencoded::parse(uri.query().unwrap().as_bytes())
            .into_owned()
            .collect();
    assert_eq!(fields["iri"], "https://example.org/a#b?c=d&z");
    assert_eq!(fields["expanded"], "true");
}

#[tokio::test]
async fn update_form_preserves_exact_sparql_and_provenance_and_request_identity() {
    let server = McpServer::new(Router::new().route("/update", post(echo)));
    let update = "INSERT DATA { GRAPH <urn:test:g> { <urn:s> <urn:p> \"a+b&c=é\" } }";
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer fixture-only".parse().unwrap());
    headers.insert("x-quipu-task", "fixture-task".parse().unwrap());
    let result = payload(
        server
            .dispatch(
                "quipu_update",
                json!({"update":update,"actor":"urn:declared:actor","source":"fixture+source"}),
                &headers,
            )
            .await
            .unwrap(),
    );
    assert_eq!(result["type"], "application/x-www-form-urlencoded");
    let fields: std::collections::BTreeMap<_, _> =
        url::form_urlencoded::parse(result["body"].as_str().unwrap().as_bytes())
            .into_owned()
            .collect();
    assert_eq!(fields["update"], update);
    assert_eq!(fields["actor"], "urn:declared:actor");
    assert_eq!(fields["source"], "fixture+source");
    assert_eq!(result["authorization"], "Bearer fixture-only");
    assert_eq!(result["task"], "fixture-task");
}

#[tokio::test]
async fn wrong_scope_type_unknown_fields_and_missing_scope_refuse_before_dispatch() {
    let server = McpServer::new(
        Router::new()
            .route("/share", post(echo))
            .route("/reason", post(echo)),
    );
    for (name, input) in [
        ("quipu_share", json!({})),
        ("quipu_share", json!({"scope":{"kind":"graph","value":45}})),
        (
            "quipu_share",
            json!({"scope":{"kind":"root"},"destination":"internal"}),
        ),
        ("quipu_reason", json!({"graph":false})),
        ("quipu_reason", json!({"graph_iri":"urn:g"})),
    ] {
        assert!(
            server
                .dispatch(name, input, &HeaderMap::new())
                .await
                .is_err()
        );
    }
    let result = payload(
        server
            .dispatch(
                "quipu_share",
                json!({"scope":{"kind":"graph","value":"urn:g"}}),
                &HeaderMap::new(),
            )
            .await
            .unwrap(),
    );
    assert!(result["body"].as_str().unwrap().contains("urn:g"));
}

#[test]
fn write_annotations_cover_new_workflows_and_reads_stay_read_only() {
    let server = McpServer::new(Router::new());
    for (name, write) in [
        ("quipu_share", false),
        ("quipu_import", true),
        ("quipu_import_promote", true),
        ("quipu_update", true),
        ("quipu_reason", true),
        ("quipu_events", false),
        ("quipu_events_commit", true),
        ("quipu_changes", false),
        ("quipu_subscriptions", true),
        ("quipu_embed_backfill", true),
        ("quipu_knot_stage", true),
        ("quipu_knot_promote", true),
    ] {
        let tool = server.tools.iter().find(|t| t.name == name).unwrap();
        let definition = serde_json::to_value(tool).unwrap();
        assert_eq!(definition["annotations"]["readOnlyHint"], !write, "{name}");
    }
}
