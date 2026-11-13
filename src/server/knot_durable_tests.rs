//! V2 HTTP controls. These require the server feature build, separately from
//! the native ledger qualification; they never contact a production service.
use axum::{Router, body::Body, http::Request};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

const ID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
fn input() -> Value {
    json!({"turtle":"<urn:durable:test> <http://www.w3.org/2000/01/rdf-schema#label> \"retained\" ."})
}
fn identity(name: &str) -> quipu::transaction_auth::Identity {
    quipu::transaction_auth::Identity {
        principal: format!("urn:durable:{name}"),
        credential_id: Some(name.into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}
fn app(store: quipu::Store) -> Router {
    crate::snapshot_upload::routes().with_state(Arc::new(crate::StoreHandle::writer_only(store)))
}
async fn call(app: Router, path: &str, body: Value, v2: bool, who: Option<&str>) -> (u16, Value) {
    let mut req = Request::post(path).header("content-type", "application/json");
    if path == "/knot" {
        req = req.header("x-quipu-write-id", ID);
        if v2 {
            req = req.header("x-quipu-completion-contract", crate::knot_durable::CONTRACT);
        }
    }
    let response = crate::auth::run_identified(
        who.map(identity),
        app.oneshot(req.body(Body::from(body.to_string())).unwrap()),
    )
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
fn status(request: &Value) -> Value {
    json!({"contract":crate::knot_durable::CONTRACT,"write_id":ID,"request":request})
}

#[tokio::test]
async fn durable_http_receipt_survives_store_reopen_and_refuses_legacy_downgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let first = app(quipu::Store::open(path.to_str().unwrap()).unwrap());
    let (code, result) = call(first.clone(), "/knot", input(), true, Some("alice")).await;
    assert_eq!(code, 200);
    drop(first);
    let reopened = app(quipu::Store::open(path.to_str().unwrap()).unwrap());
    let (code, proof) = call(
        reopened.clone(),
        "/knot/status",
        status(&input()),
        false,
        Some("alice"),
    )
    .await;
    assert_eq!(code, 200);
    assert_eq!(proof["state"], "finished");
    assert_eq!(proof["receipt"]["tx_id"], result["tx_id"]);
    assert_eq!(
        call(reopened.clone(), "/knot", input(), true, Some("alice"))
            .await
            .0,
        400
    );
    assert_eq!(
        call(reopened, "/knot", input(), false, Some("alice"))
            .await
            .0,
        400
    );
}

#[tokio::test]
async fn durable_status_body_or_authenticated_owner_drift_is_unknown() {
    let app = app(quipu::Store::open_in_memory().unwrap());
    assert_eq!(
        call(app.clone(), "/knot", input(), true, Some("alice"))
            .await
            .0,
        200
    );
    let (_, foreign) = call(
        app.clone(),
        "/knot/status",
        status(&input()),
        false,
        Some("bob"),
    )
    .await;
    assert_eq!(foreign["state"], "unknown");
    assert!(foreign.get("receipt").is_none());
    let changed = json!({"turtle":"<urn:changed> <urn:p> <urn:o> ."});
    let (_, drift) = call(app, "/knot/status", status(&changed), false, Some("alice")).await;
    assert_eq!(drift["state"], "unknown");
    assert!(drift.get("receipt").is_none());
}

#[tokio::test]
async fn durable_admission_refuses_anonymous_identity_without_effect() {
    let app = app(quipu::Store::open_in_memory().unwrap());
    assert_eq!(call(app.clone(), "/knot", input(), true, None).await.0, 400);
    let (_, unknown) = call(app, "/knot/status", status(&input()), false, Some("alice")).await;
    assert_eq!(unknown["state"], "unknown");
}

fn guarded(state: crate::SharedStore, read_only: bool) -> Router {
    use sha2::{Digest, Sha256};
    let registry = json!({"version":1,"credentials":[{"credential_id":"alice","principal":"urn:durable:alice","audience":"quipu","token_sha256":format!("{:x}",Sha256::digest(b"fixture-alice"))}]});
    let named = quipu::crew_credentials::CredentialRegistry::parse(
        &serde_json::to_vec(&registry).unwrap(),
        "quipu",
    )
    .unwrap();
    crate::auth_guard::layer(
        crate::snapshot_upload::routes(),
        crate::auth_guard::Config {
            auth_policy: quipu::http_auth::BearerPolicy::new(
                Some("fixture-shared".into()),
                None,
                None,
                1,
            )
            .unwrap()
            .with_named(named),
            session_keys: Default::default(),
            attest_store: state.clone(),
            replica_config: None,
            read_only,
            is_relay: false,
            scoped_enabled: false,
        },
    )
    .with_state(state)
}
fn request(path: &str, body: &Value, token: Option<&str>) -> Request<Body> {
    let mut r = Request::post(path).header("content-type", "application/json");
    if let Some(token) = token {
        r = r.header("authorization", format!("Bearer {token}"));
    }
    if path == "/knot" {
        r = r
            .header("x-quipu-write-id", ID)
            .header("x-quipu-completion-contract", crate::knot_durable::CONTRACT);
    }
    r.body(Body::from(body.to_string())).unwrap()
}

#[tokio::test]
async fn durable_real_bearer_and_read_only_middleware_remain_enforced() {
    let state = Arc::new(crate::StoreHandle::writer_only(
        quipu::Store::open_in_memory().unwrap(),
    ));
    let writer = guarded(state.clone(), false);
    assert_eq!(
        writer
            .clone()
            .oneshot(request("/knot", &input(), Some("fixture-alice")))
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        writer
            .clone()
            .oneshot(request("/knot/status", &status(&input()), None))
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        writer
            .oneshot(request(
                "/knot/status",
                &status(&input()),
                Some("wrong-fixture")
            ))
            .await
            .unwrap()
            .status(),
        401
    );
    let reader = guarded(state, true);
    let proof = reader
        .clone()
        .oneshot(request(
            "/knot/status",
            &status(&input()),
            Some("fixture-alice"),
        ))
        .await
        .unwrap();
    assert_eq!(proof.status(), 200);
    let bytes = axum::body::to_bytes(proof.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["state"],
        "finished"
    );
    assert_eq!(
        reader
            .oneshot(request("/knot", &input(), Some("fixture-alice")))
            .await
            .unwrap()
            .status(),
        403
    );
}

#[test]
fn durable_process_child() {
    let Ok(mode) = std::env::var("QUIPU_KNOT_TEST_CHILD") else {
        return;
    };
    let path = std::env::var("QUIPU_KNOT_TEST_DB").unwrap();
    let mut store = quipu::Store::open(&path).unwrap();
    if mode == "commit-gap" || mode == "admission-gap" {
        quipu::transaction_auth::with_identity(Some(identity("alice")), || {
            let binding =
                quipu::store::knot_completion::Binding::for_request(ID.into(), &input()).unwrap();
            let ticket = store.admit_knot_attempt(binding).unwrap();
            if mode == "commit-gap" {
                quipu::mcp::knot::tool_knot_with_completion(&mut store, &input(), &ticket).unwrap();
            }
            // Deliberately skip worker finalization and Store Drop. The parent
            // opens the durable DB in a different OS process after this exit.
            std::process::exit(0);
        });
    }
    assert_eq!(mode, "lost-response");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let router = guarded(Arc::new(crate::StoreHandle::writer_only(store)), false);
        let response = router
            .oneshot(request("/knot", &input(), Some("fixture-alice")))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        drop(response); // The caller never consumes the HTTP response body.
    });
}

#[tokio::test]
async fn durable_process_restart_preserves_lost_response_and_crash_gap_inverses() {
    for (mode, expected) in [
        ("lost-response", "finished"),
        ("commit-gap", "committed"),
        ("admission-gap", "admitted"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.db");
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "knot_durable::tests::durable_process_child",
                "--nocapture",
            ])
            .env("QUIPU_KNOT_TEST_CHILD", mode)
            .env("QUIPU_KNOT_TEST_DB", &path)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "child fixture failed: {}",
            String::from_utf8_lossy(&child.stderr)
        );
        let reopened = app(quipu::Store::open(path.to_str().unwrap()).unwrap());
        let (code, proof) = call(
            reopened.clone(),
            "/knot/status",
            status(&input()),
            false,
            Some("alice"),
        )
        .await;
        assert_eq!(code, 200);
        assert_eq!(proof["state"], expected, "{mode}");
        assert_eq!(proof.get("commit").is_some(), mode != "admission-gap");
        if mode == "admission-gap" {
            quipu::transaction_auth::with_identity(Some(identity("alice")), || {
                let mut store = quipu::Store::open(path.to_str().unwrap()).unwrap();
                let binding =
                    quipu::store::knot_completion::Binding::for_request(ID.into(), &input())
                        .unwrap();
                assert_eq!(
                    store.seal_knot_binding(&binding).unwrap()["state"],
                    "sealed"
                );
            });
        }
        assert_eq!(
            call(reopened, "/knot", input(), true, Some("alice"))
                .await
                .0,
            400
        );
    }
}

#[cfg(feature = "reactive-reasoner")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_http_cancellation_after_commit_does_not_fabricate_finished() {
    use std::sync::Mutex;
    struct Held {
        started: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl quipu::store::TransactObserver for Held {
        fn after_commit(
            &self,
            store: &mut quipu::Store,
            _: &quipu::store::Delta,
        ) -> quipu::Result<()> {
            if let Some(started) = self.started.lock().unwrap().take() {
                // Existing native test verifies autocommit here as well.
                let _ = store.store_id()?;
                started.send(()).unwrap();
                self.release
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
            }
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let mut store = quipu::Store::open(path.to_str().unwrap()).unwrap();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    store.add_observer(Arc::new(Held {
        started: Mutex::new(Some(started_tx)),
        release: Mutex::new(release_rx),
    }));
    let work = tokio::spawn(call(app(store), "/knot", input(), true, Some("alice")));
    started_rx.await.unwrap();
    let reader = app(quipu::Store::open_read_only(path.to_str().unwrap()).unwrap());
    let (_, committed) = call(
        reader.clone(),
        "/knot/status",
        status(&input()),
        false,
        Some("alice"),
    )
    .await;
    assert_eq!(committed["state"], "committed");
    assert!(committed.get("receipt").is_none());
    work.abort();
    assert!(work.await.unwrap_err().is_cancelled());
    release_tx.send(()).unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let (_, proof) = call(
                reader.clone(),
                "/knot/status",
                &status(&input()),
                false,
                Some("alice"),
            )
            .await;
            if proof["state"] == "finished" {
                break proof;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(completed["receipt"]["tx_id"].is_u64());
}

#[tokio::test]
async fn retained_legacy_id_cannot_be_upgraded_into_a_second_execution() {
    let app = app(quipu::Store::open_in_memory().unwrap());
    assert_eq!(
        call(app.clone(), "/knot", input(), false, Some("alice"))
            .await
            .0,
        200
    );
    assert_eq!(
        call(app, "/knot", input(), true, Some("alice")).await.0,
        400
    );
}
