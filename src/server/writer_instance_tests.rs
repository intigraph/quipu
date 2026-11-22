//! Actual HTTP handler plus the production bearer middleware, on private stores.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

fn fixture(token: Option<&str>, read_only: bool) -> (crate::SharedStore, Router) {
    let store = quipu::Store::open_in_memory().unwrap();
    store.graph_create("urn:instance:known").unwrap();
    let state = Arc::new(crate::StoreHandle::writer_only(store));
    let app = crate::auth_guard::layer(
        super::routes(),
        crate::auth_guard::Config {
            auth_policy: quipu::http_auth::BearerPolicy::new(
                token.map(str::to_owned),
                None,
                None,
                1,
            )
            .unwrap(),
            session_keys: Default::default(),
            attest_store: state.clone(),
            replica_config: None,
            read_only,
            is_relay: false,
            scoped_enabled: false,
        },
    )
    .with_state(state.clone());
    (state, app)
}

async fn call(app: Router, query: &str, token: Option<&str>) -> (u16, Value) {
    let mut request = Request::get(format!("/graph/writer-instance?{query}"));
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn snapshot(state: &crate::SharedStore) -> (i64, (u64, u64, u64)) {
    let store = state.read();
    (store.latest_tx_id().unwrap(), store.graph_counts().unwrap())
}

#[tokio::test]
async fn writer_instance_authentication_precedes_graph_existence() {
    for token in [None, Some("wrong")] {
        for graph in ["urn:instance:known", "urn:instance:unknown"] {
            let (_, app) = fixture(Some("fixture-only"), false);
            assert_eq!(call(app, &format!("graph={graph}"), token).await.0, 401);
        }
    }
    let (_, app) = fixture(None, false);
    assert_eq!(call(app, "graph=urn:instance:known", None).await.0, 401);
    let (state, _) = fixture(Some("fixture-only"), false);
    assert_eq!(
        call(
            super::routes().with_state(state),
            "graph=urn:instance:known",
            None
        )
        .await
        .0,
        401
    );
}

#[tokio::test]
async fn writer_instance_http_equals_owner_primitive_and_does_not_write() {
    for read_only in [false, true] {
        let (state, app) = fixture(Some("fixture-only"), read_only);
        let expected = serde_json::json!(
            state
                .read()
                .writer_graph_instance("urn:instance:known")
                .unwrap()
        );
        let before = snapshot(&state);
        assert!(
            state
                .read()
                .lookup("urn:instance:unknown")
                .unwrap()
                .is_none()
        );
        for _ in 0..2 {
            let (status, value) = call(
                app.clone(),
                "graph=urn:instance:known",
                Some("fixture-only"),
            )
            .await;
            assert_eq!(status, 200);
            assert_eq!(value, expected);
            assert_eq!(value.as_object().unwrap().len(), 3);
        }
        assert_eq!(
            call(
                app.clone(),
                "graph=urn:instance:unknown",
                Some("fixture-only")
            )
            .await
            .0,
            403
        );
        let root = format!("graph={}", quipu::schema::ROOT_GRAPH_IRI);
        assert_eq!(call(app, &root, Some("fixture-only")).await.0, 403);
        assert_eq!(before, snapshot(&state));
        assert!(
            state
                .read()
                .lookup("urn:instance:unknown")
                .unwrap()
                .is_none()
        );
        assert!(!quipu::http_auth::is_write_request(
            "/graph/writer-instance",
            "GET"
        ));
    }
}

#[tokio::test]
async fn writer_instance_recreation_changes_the_actual_pin() {
    let (state, app) = fixture(Some("fixture-only"), false);
    let (_, old) = call(
        app.clone(),
        "graph=urn:instance:known",
        Some("fixture-only"),
    )
    .await;
    {
        let store = state.lock();
        let id = store
            .committed_graph_id("urn:instance:known")
            .unwrap()
            .unwrap();
        store.graph_unregister(id).unwrap();
        store.graph_create("urn:instance:known").unwrap();
    }
    let (status, new) = call(app, "graph=urn:instance:known", Some("fixture-only")).await;
    assert_eq!(status, 200);
    assert_eq!(old["store_id"], new["store_id"]);
    assert_eq!(old["graph"], new["graph"]);
    assert_ne!(old["generation"], new["generation"]);
}

#[tokio::test]
async fn writer_instance_selector_is_bounded_and_closed() {
    let (_, app) = fixture(Some("fixture-only"), false);
    for query in [
        "",
        "graph=urn:instance:known&extra=unexpected",
        &format!("graph={}", "a".repeat(2049)),
    ] {
        assert_eq!(call(app.clone(), query, Some("fixture-only")).await.0, 400);
    }
}

#[tokio::test]
async fn writer_instance_restricted_credentials_cannot_become_owner_reads() {
    use sha2::{Digest, Sha256};
    for replication in [false, true] {
        let (state, _) = fixture(Some("fixture-only"), false);
        let bytes = serde_json::to_vec(&serde_json::json!({"version":1,"credentials":[{
            "credential_id":"restricted-fixture", "principal":"urn:restricted:fixture",
            "audience":"quipu", "token_sha256":format!("{:x}", Sha256::digest(b"restricted-fixture"))
        }]})).unwrap();
        let registry = quipu::crew_credentials::CredentialRegistry::parse(&bytes, "quipu").unwrap();
        let policy =
            quipu::http_auth::BearerPolicy::new(Some("fixture-only".into()), None, None, 1)
                .unwrap();
        let policy = if replication {
            policy.with_replication_only(registry)
        } else {
            policy.with_scoped_writers(registry)
        }
        .unwrap();
        let app = crate::auth_guard::layer(
            super::routes(),
            crate::auth_guard::Config {
                auth_policy: policy,
                session_keys: Default::default(),
                attest_store: state.clone(),
                replica_config: None,
                read_only: false,
                is_relay: false,
                scoped_enabled: false,
            },
        )
        .with_state(state);
        assert!(matches!(
            call(app, "graph=urn:instance:known", Some("restricted-fixture"))
                .await
                .0,
            401 | 403
        ));
    }
}

#[tokio::test]
async fn writer_instance_refuses_overlay_and_frozen_targets() {
    let (state, app) = fixture(Some("fixture-only"), false);
    {
        let store = state.lock();
        let parent = store
            .committed_graph_id("urn:instance:known")
            .unwrap()
            .unwrap();
        store
            .overlay_create("urn:instance:overlay", parent)
            .unwrap();
    }
    assert_eq!(
        call(
            app.clone(),
            "graph=urn:instance:overlay",
            Some("fixture-only")
        )
        .await
        .0,
        403
    );
    let directory = tempfile::tempdir().unwrap();
    state
        .lock()
        .freeze_graph(
            "urn:instance:known",
            directory.path().to_str().unwrap(),
            "2026-01-01T00:00:00Z",
            None,
        )
        .unwrap();
    let before = snapshot(&state);
    assert_eq!(
        call(app, "graph=urn:instance:known", Some("fixture-only"))
            .await
            .0,
        403
    );
    assert_eq!(snapshot(&state), before);
}
