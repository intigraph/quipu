//! Shared query routes; typed answers do not alter the existing SPARQL route.

use crate::{SharedStore, admission::read_blocking, base::AppError};
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};

pub(crate) fn routes() -> Router<SharedStore> {
    Router::new()
        .route("/query", get(super::query_get).post(super::query_post_http))
        .route("/rule_query", post(rule_query))
}

async fn rule_query(
    State(store): State<SharedStore>,
    Json(request): Json<quipu::rule_answers::Request>,
) -> Result<Json<quipu::rule_answers::Answer>, AppError> {
    read_blocking(move || Ok(Json(quipu::rule_answers::query(&store.read(), &request)?))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use serde_json::json;
    use tower::ServiceExt;

    #[tokio::test]
    async fn rule_answer_states_actual_http_matches_readonly_native_and_refuses_duplicate_fields() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("answers.db");
        let path = file.to_str().unwrap();
        let mut store = quipu::Store::open(path).unwrap();
        store
            .load_shapes(
                "rule-answers",
                include_str!("../../shapes/rule-answers.ttl"),
                "2026-01-01",
            )
            .unwrap();
        quipu::tool_knot(&mut store, &json!({"turtle":
            "<urn:supported> <urn:positive> <urn:target> . <urn:refuted> <urn:negative> <urn:target> . <urn:both> <urn:positive> <urn:target> ; <urn:negative> <urn:target> ."
        })).unwrap();
        let reader = quipu::Store::open_read_only(path).unwrap();
        let shared = std::sync::Arc::new(crate::StoreHandle::writer_only(reader));
        let app = routes().with_state(shared);
        for subject in ["supported", "refuted", "both", "neither"] {
            let request = json!({"subject":format!("urn:{subject}"),"positive_predicate":"urn:positive",
                "negative_predicate":"urn:negative","object":{"iri":"urn:target"},"judgment_policy":"urn:policy"});
            let response = app
                .clone()
                .oneshot(
                    Request::post("/rule_query")
                        .header("content-type", "application/json")
                        .body(Body::from(request.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap();
            let wire: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(wire, quipu::tool_rule_query(&store, &request).unwrap());
        }
        let duplicate = r#"{"subject":"urn:a","subject":"urn:b","positive_predicate":"urn:p","negative_predicate":"urn:n","object":{"iri":"urn:o"},"judgment_policy":"urn:policy"}"#;
        let response = app
            .oneshot(
                Request::post("/rule_query")
                    .header("content-type", "application/json")
                    .body(Body::from(duplicate))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}

#[cfg(test)]
mod profile_tests {
    use axum::{body::Body, http::Request};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    #[tokio::test]
    async fn execution_profile_http_is_opt_in_and_refuses_uncovered_transports() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.db");
        let store = quipu::Store::open(path.to_str().unwrap()).unwrap();
        let shared = std::sync::Arc::new(crate::StoreHandle::writer_only(store));
        let app = super::routes().with_state(shared);
        let query = "SELECT ?x WHERE { VALUES ?x { 1 2 } FILTER(?x = 1) }";
        let mut answers = Vec::new();
        for enabled in [false, true] {
            let response = app
                .clone()
                .oneshot(
                    Request::post("/query")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({"query":query,"execution_profile":enabled}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let bytes = axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap();
            let mut body: Value = serde_json::from_slice(&bytes).unwrap();
            let profile = body.as_object_mut().unwrap().remove("execution_profile");
            assert_eq!(profile.is_some(), enabled);
            if let Some(profile) = profile {
                assert_eq!(profile["contract"], "quipu-execution-profile/v1");
                assert!(!profile["operators"].as_array().unwrap().is_empty());
            }
            assert!(body.get("ignored_fields").is_none(), "{body}");
            answers.push(body);
        }
        assert_eq!(answers[0], answers[1]);
        for extra in [
            json!({"federated":true}),
            json!({"_sparql_protocol":true}),
            json!({}),
        ] {
            let mut input = json!({"query":query,"execution_profile":true});
            input
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let response = app
                .clone()
                .oneshot(
                    Request::post("/query")
                        .header("content-type", "application/json")
                        .header(
                            "accept",
                            if extra == json!({}) {
                                "application/sparql-results+json"
                            } else {
                                "application/json"
                            },
                        )
                        .body(Body::from(input.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), 400);
        }
    }
}
