//! Completion evidence must follow the worker, never the disconnected client.
use axum::{Router, body::Body, http::Request};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn input() -> Value {
    json!({"turtle":"<urn:completion:item> <http://www.w3.org/2000/01/rdf-schema#label> \"completion\" .","snapshot":"completion-test","replace_snapshot":true})
}

async fn call(app: Router, path: &str, body: Value, id: Option<&str>) -> (u16, Value) {
    call_with_bearer(app, path, body, id, None).await
}

async fn call_with_bearer(
    app: Router,
    path: &str,
    body: Value,
    id: Option<&str>,
    bearer: Option<&str>,
) -> (u16, Value) {
    let mut request = Request::post(path).header("content-type", "application/json");
    if let Some(id) = id {
        request = request.header("x-quipu-write-id", id);
    }
    if let Some(bearer) = bearer {
        request = request.header("authorization", format!("Bearer {bearer}"));
    }
    let response = app
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn setup() -> (crate::SharedStore, Router) {
    let state = Arc::new(crate::StoreHandle::writer_only(
        quipu::Store::open_in_memory().unwrap(),
    ));
    let app = crate::snapshot_upload::routes().with_state(state.clone());
    (state, app)
}

#[tokio::test]
async fn knot_completion_never_seen_is_unknown_not_not_started() {
    let (_, app) = setup();
    let (status, receipt) = call(
        app,
        "/knot/status",
        json!({"write_id":ID,"request":input()}),
        None,
    )
    .await;
    assert_eq!(status, 200, "missing native completion capability");
    assert_eq!(receipt["contract"], "quipu.knot-completion/v1");
    assert_eq!(receipt["state"], "unknown");
}

#[tokio::test]
async fn knot_completion_duplicate_id_refuses_without_reexecuting() {
    let (_, app) = setup();
    assert_eq!(call(app.clone(), "/knot", input(), Some(ID)).await.0, 200);
    let (status, _) = call(app.clone(), "/knot", input(), Some(ID)).await;
    assert_eq!(status, 400, "a retained ID must never dispatch twice");
    let (status, receipt) = call(
        app,
        "/knot/status",
        json!({"write_id":ID,"request":input()}),
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(receipt["state"], "finished");
    assert!(receipt["receipt"]["tx_id"].is_u64());
}

fn identity(name: &str) -> quipu::transaction_auth::Identity {
    quipu::transaction_auth::Identity {
        principal: format!("urn:completion:{name}"),
        credential_id: Some(name.into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}

#[tokio::test]
async fn knot_completion_foreign_identity_cannot_read_or_replace_receipt() {
    let (_, app) = setup();
    let alice = Some(identity("alice"));
    let bob = Some(identity("bob"));
    assert_eq!(
        crate::auth::run_identified(alice.clone(), call(app.clone(), "/knot", input(), Some(ID)))
            .await
            .0,
        200
    );
    let query = json!({"write_id":ID,"request":input()});
    let (_, hidden) = crate::auth::run_identified(
        bob.clone(),
        call(app.clone(), "/knot/status", query.clone(), None),
    )
    .await;
    assert_eq!(hidden["state"], "unknown");
    assert!(hidden.get("receipt").is_none());
    assert_eq!(
        crate::auth::run_identified(bob, call(app.clone(), "/knot", input(), Some(ID)))
            .await
            .0,
        400
    );
    let (_, retained) =
        crate::auth::run_identified(alice, call(app, "/knot/status", query, None)).await;
    assert_eq!(retained["state"], "finished");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn knot_completion_late_commit_outlives_cancelled_http_future() {
    let (state, app) = setup();
    let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let holder = state.clone();
    let thread = std::thread::spawn(move || {
        let _writer = holder.writer.lock();
        locked_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    locked_rx.await.unwrap();
    let mut work = tokio::spawn(call(app.clone(), "/knot", input(), Some(ID)));
    let query = json!({"write_id":ID,"request":input()});
    let observed = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let (status, receipt) = call(app.clone(), "/knot/status", query.clone(), None).await;
            if status != 200 {
                break false;
            }
            if receipt["state"] == "running" {
                break true;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    // Release even on a failed baseline, so a blocking task never hangs a test.
    if !matches!(observed, Ok(true)) {
        release_tx.send(()).unwrap();
        thread.join().unwrap();
        let _ = work.await;
        panic!("missing running receipt while writer is blocked: {observed:?}");
    }
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(1), &mut work)
            .await
            .is_err(),
        "client deadline must expire before releasing the writer"
    );
    work.abort();
    assert!(work.await.unwrap_err().is_cancelled());
    let (_, running) = call(app.clone(), "/knot/status", query.clone(), None).await;
    assert_eq!(
        running["state"], "running",
        "HTTP cancellation is not completion"
    );
    release_tx.send(()).unwrap();
    thread.join().unwrap();
    let receipt = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let (_, receipt) = call(app.clone(), "/knot/status", query.clone(), None).await;
            if receipt["state"] == "finished" {
                break receipt;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(receipt["receipt"]["tx_id"].is_u64());
    assert_eq!(state.lock().graph_counts().unwrap().1, 1);
}

#[tokio::test]
async fn knot_completion_missing_or_malformed_binding_never_proves_completion() {
    let (_, app) = setup();
    for body in [
        json!({}),
        json!({"write_id":ID}),
        json!({"write_id":3,"request":input()}),
    ] {
        assert_ne!(call(app.clone(), "/knot/status", body, None).await.0, 200);
    }
    assert_ne!(
        call(app.clone(), "/knot", input(), Some("malformed"))
            .await
            .0,
        200
    );
    assert_eq!(
        call(app, "/knot", input(), None).await.0,
        200,
        "ordinary caller compatibility"
    );
}

#[tokio::test]
async fn knot_completion_real_bearer_lookup_preserves_read_only_and_auth_guards() {
    let (state, _) = setup();
    let guarded = |read_only| {
        crate::auth_guard::layer(
            crate::snapshot_upload::routes(),
            crate::auth_guard::Config {
                auth_policy: quipu::http_auth::BearerPolicy::new(
                    Some("fixture-completion-token".into()),
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
        .with_state(state.clone())
    };
    let app = guarded(false);
    assert_eq!(
        call_with_bearer(
            app.clone(),
            "/knot",
            input(),
            Some(ID),
            Some("fixture-completion-token")
        )
        .await
        .0,
        200
    );
    let query = json!({"write_id":ID,"request":input()});
    assert_eq!(
        call(app.clone(), "/knot/status", query.clone(), None)
            .await
            .0,
        401
    );
    assert_eq!(
        call_with_bearer(
            app,
            "/knot/status",
            query.clone(),
            None,
            Some("wrong-fixture-token")
        )
        .await
        .0,
        401
    );
    let (status, receipt) = call_with_bearer(
        guarded(true),
        "/knot/status",
        query,
        None,
        Some("fixture-completion-token"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(receipt["state"], "finished");
    assert_eq!(
        call_with_bearer(
            guarded(true),
            "/knot",
            input(),
            Some(ID),
            Some("fixture-completion-token")
        )
        .await
        .0,
        403
    );
    assert!(!quipu::http_auth::is_write_request("/knot/status", "POST"));
}

#[tokio::test]
async fn knot_completion_wrong_body_and_failed_native_write_do_not_claim_commit() {
    let (state, app) = setup();
    let invalid = json!({"turtle":"this is not Turtle"});
    assert_ne!(
        call(app.clone(), "/knot", invalid.clone(), Some(ID))
            .await
            .0,
        200
    );
    let (_, stopped) = call(
        app.clone(),
        "/knot/status",
        json!({"write_id":ID,"request":invalid}),
        None,
    )
    .await;
    assert_eq!(stopped["state"], "stopped");
    assert!(stopped.get("receipt").is_none());
    assert_eq!(state.lock().graph_counts().unwrap().1, 0);
    let (_, different) = call(
        app,
        "/knot/status",
        json!({"write_id":ID,"request":input()}),
        None,
    )
    .await;
    assert_eq!(different["state"], "unknown");
}
