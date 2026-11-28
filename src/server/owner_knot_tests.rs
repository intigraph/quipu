use super::*;
use quipu::{
    Store,
    graph_catalog::{Budget, Catalog, Route},
    store::graph_owner::Owner,
};
use serde_json::json;
use std::time::Duration;

const GRAPH: &str = "https://example.test/graphs/transport";
const ID: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn identity(class: &str) -> quipu::transaction_auth::Identity {
    quipu::transaction_auth::Identity {
        principal: "urn:test:transport".into(),
        credential_id: Some("transport-key".into()),
        auth_class: class.into(),
        attestation: Default::default(),
    }
}

fn fixture() -> (tempfile::TempDir, Store, Arc<PreparedOwner>, Pin, Value) {
    fixture_with_scoped_grant(false)
}

fn fixture_with_scoped_grant(
    granted: bool,
) -> (tempfile::TempDir, Store, Arc<PreparedOwner>, Pin, Value) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("owner.db");
    Store::open(path.to_str().unwrap())
        .unwrap()
        .graph_create(GRAPH)
        .unwrap();
    Store::adopt(path.to_str().unwrap()).unwrap();
    let store = Store::open(path.to_str().unwrap()).unwrap();
    if granted {
        use quipu::writer_scope::{WriteGrant, WriteOperation};
        store
            .writer_grant_apply(&WriteGrant {
                version: 1,
                id: "transport-grant".into(),
                principal: "urn:test:transport".into(),
                credential_id: "transport-key".into(),
                instance: store.writer_graph_instance(GRAPH).unwrap(),
                operations: std::collections::BTreeSet::from([WriteOperation::Knot]),
                valid_from: 0,
                valid_until: quipu::time::epoch_secs() + 3600,
                revision: 1,
                revoked: false,
                mint_scope: None,
            })
            .unwrap();
    }
    let route = Route {
        graph: GRAPH.into(),
        file_id: "owner".into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation: 4,
        policy: "policy-v1".into(),
    };
    let legacy = Store::open(root.join("legacy.db").to_str().unwrap()).unwrap();
    let catalog = Arc::new(
        Catalog::new(
            7,
            &legacy.store_id().unwrap(),
            vec![route],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 3,
            },
        )
        .unwrap(),
    );
    legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    let owner = Owner::prepare(catalog.clone(), &root, GRAPH, store, "policy-v1").unwrap();
    let router = Router::admit(catalog, &legacy, vec![owner]).unwrap();
    let endpoint = PreparedOwner::new(router, GRAPH.into()).unwrap();
    let pin = Pin {
        catalog_revision: 7,
        generation: 4,
        policy: "policy-v1".into(),
        signed: false,
        cross_graph_context: false,
    };
    let input = json!({"graph":GRAPH,"turtle":"<urn:test:s> <urn:test:p> \"transport\" .","source":"transport-test"});
    (dir, legacy, endpoint, pin, input)
}

#[tokio::test]
async fn real_owner_receipt_progresses_with_legacy_writer_and_global_permit_held() {
    let (dir, legacy, owner, pin, input) = fixture();
    let sqlite = rusqlite::Connection::open(dir.path().join("legacy.db")).unwrap();
    sqlite.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, held) = std::sync::mpsc::channel();
    let legacy_work = tokio::spawn(crate::admission::write_blocking(move || {
        started.send(()).unwrap();
        held.recv().unwrap();
        Ok(())
    }));
    running.await.unwrap();
    let outcome = crate::auth::run_identified(
        Some(identity("named_bearer")),
        tokio::time::timeout(
            Duration::from_secs(5),
            owner.knot(pin.clone(), input.clone(), ID.into()),
        ),
    )
    .await;
    sqlite.execute_batch("ROLLBACK").unwrap();
    release.send(()).unwrap();
    legacy_work.await.unwrap().unwrap();
    let result = outcome
        .expect("owner entered legacy admission or SQLite writer")
        .unwrap();
    let proof = crate::auth::run_identified(
        Some(identity("named_bearer")),
        owner.status(pin.clone(), input.clone(), ID.into()),
    )
    .await
    .unwrap();
    assert_eq!(proof["state"], "finished");
    assert_eq!(proof["receipt"]["tx_id"], result["tx_id"]);
    assert!(
        crate::auth::run_identified(
            Some(identity("named_bearer")),
            owner.seal(pin.clone(), input.clone(), ID.into())
        )
        .await
        .is_err(),
        "a committed effect cannot be sealed"
    );
    assert!(
        crate::auth::run_identified(
            Some(identity("named_bearer")),
            owner.knot(pin, input, ID.into())
        )
        .await
        .is_err()
    );
    let legacy_ledger: i64 = sqlite
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='knot_attempts'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(legacy_ledger, 0);
    legacy.verify_routed_fence().unwrap();
}

#[tokio::test]
async fn status_uses_its_reader_while_owner_write_queue_is_held() {
    let (_dir, _legacy, owner, pin, input) = fixture();
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, held) = std::sync::mpsc::channel();
    let work = {
        let owner = owner.clone();
        tokio::spawn(async move {
            owner
                .writes
                .write(move || {
                    started.send(()).unwrap();
                    held.recv().unwrap();
                    Ok(())
                })
                .await
        })
    };
    running.await.unwrap();
    let proof = crate::auth::run_identified(
        Some(identity("named_bearer")),
        tokio::time::timeout(Duration::from_secs(5), owner.status(pin, input, ID.into())),
    )
    .await;
    release.send(()).unwrap();
    work.await.unwrap().unwrap();
    assert_eq!(
        proof.expect("status entered owner writer queue").unwrap()["state"],
        "unknown"
    );
}

#[tokio::test]
async fn invalid_contracts_refuse_before_consuming_an_id() {
    let (_dir, _legacy, owner, pin, input) = fixture();
    assert!(
        owner
            .knot(pin.clone(), input.clone(), ID.into())
            .await
            .is_err()
    );
    assert!(
        crate::auth::run_identified(
            Some(identity("scoped_named_bearer")),
            owner.knot(pin.clone(), input.clone(), ID.into())
        )
        .await
        .is_err()
    );
    for variant in 0..4 {
        let mut bad = pin.clone();
        match variant {
            0 => bad.generation += 1,
            1 => bad.catalog_revision += 1,
            2 => bad.signed = true,
            _ => bad.cross_graph_context = true,
        }
        assert!(
            crate::auth::run_identified(
                Some(identity("named_bearer")),
                owner.knot(bad, input.clone(), ID.into())
            )
            .await
            .is_err()
        );
    }
    let mut wrong_graph = input.clone();
    wrong_graph["graph"] = json!("urn:test:other");
    assert!(
        crate::auth::run_identified(
            Some(identity("named_bearer")),
            owner.knot(pin.clone(), wrong_graph, ID.into())
        )
        .await
        .is_err()
    );
    crate::auth::run_identified(
        Some(identity("named_bearer")),
        owner.knot(pin, input, ID.into()),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn http_adapter_routes_owner_receipts_and_refuses_missing_pins() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let (_dir, _legacy, owner, _pin, input) = fixture();
    let app = super::http::routes(owner);
    let request = |path: &str, body: Value, pins: bool| {
        let mut r = Request::post(path)
            .header("content-type", "application/json")
            .header("x-quipu-completion-contract", crate::knot_durable::CONTRACT)
            .header("x-quipu-write-id", ID);
        if pins {
            r = r
                .header("x-quipu-catalog-revision", "7")
                .header("x-quipu-owner-generation", "4")
                .header("x-quipu-owner-policy", "policy-v1");
        }
        r.body(Body::from(body.to_string())).unwrap()
    };
    let response = crate::auth::run_identified(
        Some(identity("named_bearer")),
        app.clone()
            .oneshot(request("/owner/knot", input.clone(), false)),
    )
    .await
    .unwrap();
    assert!(!response.status().is_success());
    let response = crate::auth::run_identified(
        Some(identity("named_bearer")),
        app.clone()
            .oneshot(request("/owner/knot", input.clone(), true)),
    )
    .await
    .unwrap();
    assert!(response.status().is_success(), "{}", response.status());
    let response = crate::auth::run_identified(
        Some(identity("named_bearer")),
        app.oneshot(request(
            "/owner/knot/status",
            json!({"write_id":ID,"request":input}),
            true,
        )),
    )
    .await
    .unwrap();
    assert!(response.status().is_success());
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let proof: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(proof["state"], "finished");
}

#[cfg(feature = "ownership-witness-s3")]
#[test]
fn disabled_initializer_never_resolves_invalid_config_or_credentials() {
    use quipu::store::graph_router::{EnrolledPilotConfig, IngestPilot};
    let (_dir, legacy, _owner, _pin, _input) = fixture();
    let catalog = Arc::new(
        Catalog::new(
            7,
            "legacy",
            vec![],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 4,
            },
        )
        .unwrap(),
    );
    let config = EnrolledPilotConfig {
        root: "relative".into(),
        inventory: "absent".into(),
        profile: String::new(),
        endpoint: String::new(),
        bucket: String::new(),
        region: String::new(),
        prefix: String::new(),
        timeout: Duration::ZERO,
        witness_ca_der: None,
        installed_policy: String::new(),
        allocation: quipu::memory_budget::MemoryAllocation::for_connections(1024, 4).unwrap(),
    };
    assert!(
        PreparedOwner::initialize(
            &IngestPilot::default(),
            catalog,
            &legacy,
            &config,
            || panic!("OFF resolved credentials"),
            |_| panic!("OFF configured owner")
        )
        .unwrap()
        .is_none()
    );
}

/// Exercise the real bearer classifier, scoped middleware and durable owner
/// together. Synthetic credentials and temporary owner/legacy files only.
#[tokio::test]
async fn scoped_owner_http_grant_admission_and_refusal_leave_legacy_untouched() {
    use axum::{body::Body, http::Request, middleware, response::IntoResponse};
    use quipu::{
        crew_credentials::CredentialRegistry,
        http_auth::{AccessDecision, BearerPolicy},
    };
    use sha2::{Digest, Sha256};
    use tower::ServiceExt;
    for granted in [false, true] {
        let (dir, legacy, owner, _pin, input) = fixture_with_scoped_grant(granted);
        let registry = CredentialRegistry::parse(&serde_json::to_vec(&json!({
            "version":1,"credentials":[{
                "credential_id":"transport-key","principal":"urn:test:transport",
                "audience":"quipu","token_sha256":format!("{:x}",Sha256::digest(b"fixture-owner"))
            }]
        })).unwrap(), "quipu").unwrap();
        let policy = BearerPolicy::new(Some("fixture-operator".into()), None, None, 0)
            .unwrap()
            .with_scoped_writers(registry)
            .unwrap();
        let app = super::http::routes(owner).layer(middleware::from_fn(
            move |req: axum::extract::Request, next| {
                let policy = policy.clone();
                async move {
                    let (header, authorization) =
                        crate::auth::authorize_request(&req, &policy, false);
                    if authorization.decision != AccessDecision::Allow {
                        return (axum::http::StatusCode::UNAUTHORIZED, "invalid credential")
                            .into_response();
                    }
                    crate::auth::run_authorized(
                        req,
                        next,
                        &policy,
                        authorization,
                        header.as_deref(),
                    )
                    .await
                }
            },
        ));
        let request = |path: &str, body: Value, token: &str| {
            Request::post(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .header("x-quipu-completion-contract", crate::knot_durable::CONTRACT)
                .header("x-quipu-write-id", ID)
                .header("x-quipu-catalog-revision", "7")
                .header("x-quipu-owner-generation", "4")
                .header("x-quipu-owner-policy", "policy-v1")
                .body(Body::from(body.to_string()))
                .unwrap()
        };
        // A forged/absent credential never reaches owner admission.
        let response = app
            .clone()
            .oneshot(request("/owner/knot", input.clone(), "invalid"))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
        let response = app
            .clone()
            .oneshot(request("/owner/knot", input.clone(), "fixture-owner"))
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        assert_eq!(
            status.is_success(),
            granted,
            "granted={granted}: {}",
            String::from_utf8_lossy(&bytes)
        );
        let binding = json!({"write_id":ID,"request":input});
        let response = app
            .clone()
            .oneshot(request(
                "/owner/knot/status",
                binding.clone(),
                "fixture-owner",
            ))
            .await
            .unwrap();
        assert!(response.status().is_success());
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let proof: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(proof["state"], if granted { "finished" } else { "unknown" });
        let response = app
            .oneshot(request("/owner/knot/seal", binding, "fixture-owner"))
            .await
            .unwrap();
        assert!(
            !response.status().is_success(),
            "seal needs a grant and an admitted, uncommitted attempt"
        );
        let conn = rusqlite::Connection::open(dir.path().join("owner.db")).unwrap();
        let ledger: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='knot_attempts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ledger, i64::from(granted), "refusal must consume no ID");
        let legacy_conn = rusqlite::Connection::open(dir.path().join("legacy.db")).unwrap();
        let ledger: i64 = legacy_conn
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='knot_attempts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ledger, 0);
        legacy.verify_routed_fence().unwrap();
    }
}
