//! Decision-deck HTTP routes (aegis-blh.2), off unless `[quipu.server]
//! deck_http = true`.
//!
//! Two independent switches gate a decision: this server flag, and the store's
//! operator-only `deck_config.enabled`. Either one off refuses. The handlers
//! add no authority: `present` and `attest` run the native atomic methods under
//! the writer, and `receipt` is a pooled read. Attest is never retried here.

use axum::extract::State;
use serde_json::Value as JsonValue;

use super::SharedStore;
use super::admission::{read_blocking, write_blocking};
use super::base::AppError;

fn gate(s: &SharedStore) -> Result<(), AppError> {
    if s.deck_http {
        Ok(())
    } else {
        Err(
            quipu::Error::PolicyDenied("decision-deck routes are disabled on this server".into())
                .into(),
        )
    }
}

pub(crate) async fn deck_present(
    State(s): State<SharedStore>,
    axum::Json(i): axum::Json<JsonValue>,
) -> Result<axum::Json<JsonValue>, AppError> {
    gate(&s)?;
    write_blocking(move || {
        let mut st = s.write_lock()?;
        quipu::transaction_auth::refuse_if_refused()?;
        Ok(axum::Json(quipu::tool_deck_present(&mut st, &i)?))
    })
    .await
}

pub(crate) async fn deck_attest(
    State(s): State<SharedStore>,
    axum::Json(i): axum::Json<JsonValue>,
) -> Result<axum::Json<JsonValue>, AppError> {
    gate(&s)?;
    write_blocking(move || {
        let mut st = s.write_lock()?;
        quipu::transaction_auth::refuse_if_refused()?;
        Ok(axum::Json(quipu::tool_deck_attest(&mut st, &i)?))
    })
    .await
}

pub(crate) async fn deck_receipt(
    State(s): State<SharedStore>,
    axum::Json(i): axum::Json<JsonValue>,
) -> Result<axum::Json<JsonValue>, AppError> {
    gate(&s)?;
    read_blocking(move || Ok(axum::Json(quipu::tool_deck_receipt(&s.read(), &i)?))).await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use tower::ServiceExt;

    async fn post(deck_http: bool, uri: &str) -> (axum::http::StatusCode, String) {
        let mut handle = crate::StoreHandle::writer_only(quipu::Store::open_in_memory().unwrap());
        handle.deck_http = deck_http;
        let shared: crate::SharedStore = Arc::new(handle);
        let app = axum::Router::new()
            .route("/deck/present", axum::routing::post(super::deck_present))
            .route("/deck/attest", axum::routing::post(super::deck_attest))
            .route("/deck/receipt", axum::routing::post(super::deck_receipt))
            .with_state(shared);
        let body = match uri {
            "/deck/present" => {
                json!({"request": "urn:r", "revision": 1, "credential": "dGVzdA", "choice": "approve"})
            }
            "/deck/attest" => json!({"nonce": "0".repeat(64), "client_data_json": "",
                                     "authenticator_data": "", "signature": ""}),
            _ => json!({"nonce": "0".repeat(64)}),
        };
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn every_deck_route_is_refused_while_the_server_switch_is_off() {
        for uri in ["/deck/present", "/deck/attest", "/deck/receipt"] {
            let (status, body) = post(false, uri).await;
            assert_eq!(status, axum::http::StatusCode::FORBIDDEN, "{uri}: {body}");
            assert!(body.contains("disabled on this server"), "{uri}: {body}");
        }
    }

    #[tokio::test]
    async fn the_server_switch_alone_grants_nothing() {
        // Store deck never configured: the native scope check still refuses.
        let (status, body) = post(true, "/deck/present").await;
        assert_ne!(status, axum::http::StatusCode::OK, "{body}");
        assert!(body.contains("disabled"), "{body}");
        // A read-only lookup on an unconfigured store finds nothing.
        let (status, body) = post(true, "/deck/receipt").await;
        assert_eq!(status, axum::http::StatusCode::OK, "{body}");
        assert!(body.contains("\"found\":false"), "{body}");
    }
}
