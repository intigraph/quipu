//! Authenticated HTTP pair for conditional ROOT promotion and its exact inverse.
use crate::{SharedStore, base::AppError};
use axum::{Json, extract::State};
use quipu::share_review::control;
use serde_json::Value;

enum Operation {
    Promote(control::Promote),
    Retract(control::Receipt),
}

pub(crate) async fn promote(
    State(store): State<SharedStore>,
    headers: axum::http::HeaderMap,
    principal: Option<axum::Extension<quipu::http_auth::AuthenticatedPrincipal>>,
    Json(input): Json<control::Promote>,
) -> Result<Json<Value>, AppError> {
    run(store, headers, principal, Operation::Promote(input)).await
}

pub(crate) async fn retract(
    State(store): State<SharedStore>,
    headers: axum::http::HeaderMap,
    principal: Option<axum::Extension<quipu::http_auth::AuthenticatedPrincipal>>,
    Json(input): Json<control::Receipt>,
) -> Result<Json<Value>, AppError> {
    run(store, headers, principal, Operation::Retract(input)).await
}

async fn run(
    store: SharedStore,
    headers: axum::http::HeaderMap,
    principal: Option<axum::Extension<quipu::http_auth::AuthenticatedPrincipal>>,
    operation: Operation,
) -> Result<Json<Value>, AppError> {
    let principal = principal
        .ok_or_else(|| {
            quipu::Error::InvalidValue(
                "conditional promotion requires an authenticated principal".into(),
            )
        })?
        .0;
    let client = crate::query_usage::client(&headers);
    let endpoint = match &operation {
        Operation::Promote(_) => "/import/promote/conditional",
        Operation::Retract(_) => "/import/promote/rollback",
    };
    let (started, deadline) = crate::admission::import_budget();
    crate::admission::write_blocking_until(started, deadline, move || {
        let (result, work) = {
            let waiting = std::time::Instant::now();
            let mut st = store.write_lock()?;
            let wait = waiting.elapsed().as_secs_f64();
            let acquired = std::time::Instant::now();
            let (result, profile) = st.observe_poststate(|st| {
                quipu::transaction_auth::refuse_if_refused()?;
                let ts = quipu::time::now_iso();
                let value = match operation {
                    Operation::Promote(input) => {
                        serde_json::to_value(control::promote(st, &input, principal.as_str(), &ts)?)
                    }
                    Operation::Retract(input) => {
                        serde_json::to_value(control::retract(st, &input, principal.as_str(), &ts)?)
                    }
                };
                value.map_err(|e| quipu::Error::Serialization(e.to_string()))
            });
            crate::tools::observe_write(
                &st,
                &client,
                endpoint,
                wait,
                acquired,
                result.is_ok(),
                profile,
            );
            (result?, st.take_deferred_embed())
        };
        if let Some(work) = work {
            crate::tools::finish_deferred_embed_observed(&store, &work, &client, endpoint)?;
        }
        Ok(Json(result))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Extension, Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        routing::post,
    };
    use serde_json::json;
    use std::sync::Arc;
    use tower::ServiceExt;
    const TS: &str = "2026-01-01T00:00:00Z";
    async fn call(app: Router, endpoint: &str, body: Value) -> (StatusCode, Value) {
        let response = app
            .oneshot(
                Request::post(endpoint)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }
    #[tokio::test]
    async fn paired_http_auth_promotion_and_exact_inverse() {
        let mut st = quipu::Store::open_in_memory().unwrap();
        let share = format!("sha256:{}", "a".repeat(64));
        let graph = st
            .graph_create(&format!("urn:quipu:import:staging:{}", "a".repeat(64)))
            .unwrap();
        let e = st.intern("urn:test:paired-doc").unwrap();
        let a = st.intern("urn:test:body").unwrap();
        let datum = quipu::Datum {
            entity: e,
            attribute: a,
            value: quipu::Value::Str("body".into()),
            valid_from: TS.into(),
            valid_to: None,
            op: quipu::Op::Assert,
        };
        st.transact_to_graph(&[datum], TS, None, Some("staged-fixture"), graph)
            .unwrap();
        let head = st.latest_tx_id().unwrap();
        let identity = st.store_id().unwrap();
        let before = st.current_facts().unwrap();
        let staged = st.current_facts_in_graph(graph).unwrap();
        let shared: SharedStore = Arc::new(crate::StoreHandle::writer_only(st));
        let app = Router::new()
            .route("/conditional", post(promote))
            .route("/rollback", post(retract))
            .with_state(shared.clone());
        let input = json!({"store_id":identity,"expected_head":head,"share_id":share});
        let (status, _) = call(app.clone(), "/conditional", input.clone()).await;
        assert!(!status.is_success());
        assert_eq!(shared.lock().latest_tx_id().unwrap(), head);
        let authenticated = app.layer(Extension(
            quipu::http_auth::AuthenticatedPrincipal::LEGACY_SHARED_BEARER,
        ));
        let (status, body) = call(authenticated.clone(), "/conditional", input).await;
        assert!(status.is_success(), "{body}");
        let receipt = body["receipt"].clone();
        assert!(shared.lock().current_facts().unwrap().len() > before.len());
        let (status, body) = call(authenticated.clone(), "/rollback", receipt.clone()).await;
        assert!(status.is_success(), "{body}");
        assert_eq!(shared.lock().current_facts().unwrap(), before);
        assert_eq!(shared.lock().current_facts_in_graph(graph).unwrap(), staged);
        let (status, _) = call(authenticated, "/rollback", receipt).await;
        assert!(!status.is_success());
    }
}
