//! Actual enrolled TLS/S1 owners, private policy and HTTP buffered publication.
use super::*;
use axum::http::StatusCode;
use quipu::read_authorization::{
    ComposedReadAdmission, ComposedReadReservation, OwnerReadPolicy, ReadGrant, ReadOperation,
    profile::ScopedReadConfig,
};

fn read_request(input: &Value) -> Request<Body> {
    Request::post("/query")
        .header("content-type", "application/json")
        .header("authorization", "Bearer private-fixture-token")
        .body(Body::from(input.to_string()))
        .unwrap()
}

#[tokio::test]
async fn enrolled_http_query_rechecks_credentials_grants_and_retains_response_budget() {
    let (dir, legacy, catalog, mut config, _emulator, _tls) = fixture();
    let admission = ComposedReadAdmission::new(
        catalog.clone(),
        1024,
        3,
        1,
        ComposedReadReservation {
            runtime_bytes: 16 << 20,
            view_bytes: 16 << 20,
            query_bytes: 480 << 20,
            max_response_bytes: 1 << 20,
        },
    )
    .unwrap();
    config.allocation = admission.allocation();
    let Startup::Enabled(dataset) = PreparedDataset::initialize(
        &IngestPilot {
            enabled: true,
            graph: Some(GRAPH.into()),
        },
        catalog,
        Arc::new(parking_lot::FairMutex::new(legacy)),
        &config,
        || {
            Ok((
                "AKIAIOSFODNN7EXAMPLE".into(),
                "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            ))
        },
        |_| Ok(()),
    )
    .unwrap() else {
        panic!("startup missing");
    };
    let dataset = Arc::new(*dataset);
    let response = crate::auth::run_identified(
        Some(identity("named_bearer")),
        dataset.routes().oneshot(request(
            "/owner/knot",
            &json!({
                "graph": GRAPH, "turtle": "<urn:shared> <urn:owner> \"owner\" ."
            }),
        )),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let registry = dir.path().join("registry.json");
    let registry_body = json!({"version":1,"credentials":[{
        "credential_id":"startup-key", "principal":"urn:test:startup", "audience":"quipu",
        "token_sha256":hex::encode(Sha256::digest(b"private-fixture-token"))
    }]})
    .to_string();
    fs::write(&registry, &registry_body).unwrap();
    let path = dir.path().join("policy.db");
    let mut policy = OwnerReadPolicy::open(path.to_str().unwrap(), "urn:owner").unwrap();
    let now = quipu::time::epoch_secs();
    let grant = ReadGrant {
        version: 1,
        id: "urn:grant:http".into(),
        principal: "urn:test:startup".into(),
        graphs: [quipu::schema::ROOT_GRAPH_IRI.into(), GRAPH.into()].into(),
        operations: [ReadOperation::Query].into(),
        valid_from: now - 1,
        valid_until: now + 300,
        max_offline_seconds: 30,
        issuer: "urn:owner".into(),
        revision: 1,
        revoked: false,
    };
    policy
        .apply(&grant, "private enrolled HTTP read grant")
        .unwrap();
    let read_config = ReadConfig {
        policy: ScopedReadConfig {
            policy_path: path.to_str().unwrap().into(),
            owner: "urn:owner".into(),
        },
        registry: registry.to_str().unwrap().into(),
        timeout: Duration::from_secs(10),
    };
    let mut missing = read_config.clone();
    missing.policy.policy_path = dir.path().join("absent.db").to_str().unwrap().into();
    assert!(
        dataset
            .routes_with_reads(missing, admission.clone())
            .is_err()
    );
    assert!(!dir.path().join("absent.db").exists());
    for file in ["legacy.db", "ingest.db"] {
        let mut shared = read_config.clone();
        shared.policy.policy_path = dir.path().join(file).to_str().unwrap().into();
        assert!(
            dataset
                .routes_with_reads(shared, admission.clone())
                .is_err()
        );
    }
    let mut missing_registry = read_config.clone();
    missing_registry.registry = dir
        .path()
        .join("absent-registry.json")
        .to_str()
        .unwrap()
        .into();
    assert!(
        dataset
            .routes_with_reads(missing_registry, admission.clone())
            .is_err()
    );
    let app = dataset.routes_with_reads(read_config, admission).unwrap();
    let query = json!({"graphs":[quipu::schema::ROOT_GRAPH_IRI,GRAPH],
        "query":format!("SELECT ?s ?r ?o FROM <{}> FROM NAMED <{GRAPH}> WHERE {{ ?s <urn:root> ?r . GRAPH <{GRAPH}> {{ ?s <urn:owner> ?o }} }}", quipu::schema::ROOT_GRAPH_IRI)});
    // The old write-only adapter cannot silently appear to serve these reads.
    assert_eq!(
        dataset
            .routes()
            .oneshot(read_request(&query))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND,
    );
    let run = |input: Value| {
        crate::auth::run_identified(
            Some(identity("named_bearer")),
            app.clone().oneshot(read_request(&input)),
        )
    };
    assert_eq!(
        app.clone()
            .oneshot(read_request(&query))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = run(query.clone()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(
        response.headers()["content-type"],
        "application/sparql-results+json"
    );
    // A finished computation has not finished owning its output allocation.
    assert!(!run(query.clone()).await.unwrap().status().is_success());
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["results"]["bindings"].as_array().unwrap().len(), 1);
    // Bytes itself retains the original owner until the final consumer drops it.
    assert!(!run(query.clone()).await.unwrap().status().is_success());
    drop(bytes);
    assert_eq!(run(query.clone()).await.unwrap().status(), StatusCode::OK);
    // The owner-only HTTP read must not visit the legacy writer, even when a
    // real SQLite writer owns that file. Prove the lock before the positive.
    let legacy_lock = rusqlite::Connection::open(dir.path().join("legacy.db")).unwrap();
    legacy_lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let contender = rusqlite::Connection::open(dir.path().join("legacy.db")).unwrap();
    contender.busy_timeout(Duration::ZERO).unwrap();
    assert!(contender.execute_batch("BEGIN IMMEDIATE").is_err());
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        run(json!({
            "graphs":[GRAPH],
            "query":format!("ASK {{ GRAPH <{GRAPH}> {{ <urn:shared> <urn:owner> \"owner\" }} }}")
        })),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["boolean"],
        true
    );
    drop(bytes);
    legacy_lock.execute_batch("ROLLBACK").unwrap();
    for bad in [
        json!({"query":"ASK {}","graphs":[GRAPH,GRAPH]}),
        json!({"query":"ASK {}","graphs":[]}),
        json!({"query":"ASK { GRAPH <urn:excluded> { ?s ?p ?o } }","graphs":[GRAPH]}),
        json!({"query":"ASK {}","graphs":["urn:excluded"]}),
        json!({"query":"ASK {}","graphs":[GRAPH],"principal":"urn:owner"}),
    ] {
        assert!(!run(bad).await.unwrap().status().is_success());
    }
    // Trusted outer evidence alone cannot keep a removed registry credential alive.
    fs::write(&registry, r#"{"version":1,"credentials":[]}"#).unwrap();
    assert_eq!(
        run(query.clone()).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    fs::write(&registry, registry_body).unwrap();
    let mut revoked = grant;
    revoked.revision = 2;
    revoked.revoked = true;
    policy.apply(&revoked, "revoke private grant").unwrap();
    assert_eq!(run(query).await.unwrap().status(), StatusCode::FORBIDDEN);
}
