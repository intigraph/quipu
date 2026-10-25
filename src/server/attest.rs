//! Attested-write HTTP routes (aegis-25k), off unless `[quipu.server]
//! attest_http = true`.
//!
//! Two independent switches gate an attested write: this server flag, and
//! the store's operator-only attested-write switch. Either one off refuses.
//! The handlers add no authority: `challenge` and `commit` run the native
//! atomic methods under the writer, and `receipt` is a pooled read. A commit
//! is never retried here.

use axum::extract::State;
use serde_json::Value as JsonValue;

use super::SharedStore;
use super::admission::{read_blocking, write_blocking};
use super::base::AppError;

/// The `/attest/*` routes; each refuses while the server switch is off.
pub(crate) fn routes() -> axum::Router<SharedStore> {
    axum::Router::new()
        .route("/attest/challenge", axum::routing::post(attest_challenge))
        .route("/attest/commit", axum::routing::post(attest_commit))
        .route("/attest/receipt", axum::routing::post(attest_receipt))
}

fn gate(s: &SharedStore) -> Result<(), AppError> {
    if s.attest_http {
        Ok(())
    } else {
        Err(
            quipu::Error::PolicyDenied("attested-write routes are disabled on this server".into())
                .into(),
        )
    }
}

pub(crate) async fn attest_challenge(
    State(s): State<SharedStore>,
    axum::Json(i): axum::Json<JsonValue>,
) -> Result<axum::Json<JsonValue>, AppError> {
    gate(&s)?;
    write_blocking(move || {
        let mut st = s.write_lock()?;
        quipu::transaction_auth::refuse_if_refused()?;
        Ok(axum::Json(quipu::tool_attest_challenge(&mut st, &i)?))
    })
    .await
}

pub(crate) async fn attest_commit(
    State(s): State<SharedStore>,
    axum::Json(i): axum::Json<JsonValue>,
) -> Result<axum::Json<JsonValue>, AppError> {
    gate(&s)?;
    write_blocking(move || {
        let mut st = s.write_lock()?;
        quipu::transaction_auth::refuse_if_refused()?;
        Ok(axum::Json(quipu::tool_attest_commit(&mut st, &i)?))
    })
    .await
}

pub(crate) async fn attest_receipt(
    State(s): State<SharedStore>,
    axum::Json(i): axum::Json<JsonValue>,
) -> Result<axum::Json<JsonValue>, AppError> {
    gate(&s)?;
    read_blocking(move || Ok(axum::Json(quipu::tool_attest_receipt(&s.read(), &i)?))).await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use tower::ServiceExt;

    async fn post(attest_http: bool, uri: &str) -> (axum::http::StatusCode, String) {
        let mut handle = crate::StoreHandle::writer_only(quipu::Store::open_in_memory().unwrap());
        handle.attest_http = attest_http;
        let shared: crate::SharedStore = Arc::new(handle);
        let app = axum::Router::new()
            .route(
                "/attest/challenge",
                axum::routing::post(super::attest_challenge),
            )
            .route("/attest/commit", axum::routing::post(super::attest_commit))
            .route(
                "/attest/receipt",
                axum::routing::post(super::attest_receipt),
            )
            .with_state(shared);
        let body = match uri {
            "/attest/challenge" => json!({"signer": "dGVzdA", "graph": "https://example.org/g",
                "datums": [{"s": "https://example.org/s", "p": "https://example.org/p",
                            "o": {"kind": "str", "value": "x"}}]}),
            "/attest/commit" => json!({"nonce": "0".repeat(64), "client_data_json": "",
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
    async fn every_attest_route_is_refused_while_the_server_switch_is_off() {
        for uri in ["/attest/challenge", "/attest/commit", "/attest/receipt"] {
            let (status, body) = post(false, uri).await;
            assert_eq!(status, axum::http::StatusCode::FORBIDDEN, "{uri}: {body}");
            assert!(body.contains("disabled on this server"), "{uri}: {body}");
        }
    }

    #[tokio::test]
    async fn the_server_switch_alone_grants_nothing() {
        // The store's attested-write switch was never turned on.
        for uri in ["/attest/challenge", "/attest/commit"] {
            let (status, body) = post(true, uri).await;
            assert_ne!(status, axum::http::StatusCode::OK, "{uri}: {body}");
            assert!(body.contains("disabled"), "{uri}: {body}");
        }
        // A read-only lookup on an unconfigured store finds nothing.
        let (status, body) = post(true, "/attest/receipt").await;
        assert_eq!(status, axum::http::StatusCode::OK, "{body}");
        assert!(body.contains("\"found\":false"), "{body}");
    }
}
