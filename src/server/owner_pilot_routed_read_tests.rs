//! aegis-4qv.8: the production pilot composition serves routed reads.
//! After cutover the owner's graph is readable through `POST /owner/query`
//! with the same multiset the pre-cutover legacy single file returned.
use super::*;
use crate::knot_durable::owner::startup;
use axum::http::StatusCode;
use quipu::{
    config::PilotReadsConfig,
    read_authorization::{OwnerReadPolicy, ReadGrant, ReadOperation},
    schema::ROOT_GRAPH_IRI,
    sparql, w3c,
};

const OWNER: &str = "<urn:shared> <urn:owner> \"owner\"@en, \"two\"@en, \"two\"@en; \
    <urn:link> <urn:next> . <urn:next> <urn:owner> <urn:shared> .";

fn rows(bytes: &[u8]) -> Vec<String> {
    let value: Value = serde_json::from_slice(bytes).unwrap();
    let mut rows: Vec<_> = value["results"]["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    rows.sort();
    rows
}

fn reads(dir: &Path) -> PilotReadsConfig {
    let registry = dir.join("read-registry.json");
    fs::write(
        &registry,
        json!({"version":1,"credentials":[{
            "credential_id":"startup-key", "principal":"urn:test:startup", "audience":"quipu",
            "token_sha256":hex::encode(Sha256::digest(b"routed-read-token"))
        }]})
        .to_string(),
    )
    .unwrap();
    let policy_file = dir.join("read-policy.db");
    let mut policy = OwnerReadPolicy::open(policy_file.to_str().unwrap(), "urn:owner").unwrap();
    let now = quipu::time::epoch_secs();
    policy
        .apply(
            &ReadGrant {
                version: 1,
                id: "urn:grant:routed".into(),
                principal: "urn:test:startup".into(),
                graphs: [ROOT_GRAPH_IRI.into(), GRAPH.into()].into(),
                operations: [ReadOperation::Query].into(),
                valid_from: now - 1,
                valid_until: now + 300,
                max_offline_seconds: 30,
                issuer: "urn:owner".into(),
                revision: 1,
                revoked: false,
            },
            "routed read fixture grant",
        )
        .unwrap();
    PilotReadsConfig {
        policy_file,
        owner: "urn:owner".into(),
        registry_file: registry,
        timeout_secs: 10,
        concurrency: 1,
        runtime_mib: 16,
        view_mib: 16,
        query_mib: 16,
        max_response_mib: 1,
    }
}

#[tokio::test]
async fn production_pilot_serves_owner_reads_with_pre_cutover_parity() {
    let (dir, legacy, catalog, mut config, _emulator, _tls) = fixture();
    let reads = reads(dir.path());
    // The same admission sizing and router composition `startup::mount` uses.
    let admission = startup::read_admission(&catalog, 1024, &reads).unwrap();
    config.allocation = admission.allocation();
    let Startup::Enabled(dataset) = PreparedDataset::initialize(
        &IngestPilot {
            enabled: true,
            graph: Some(GRAPH.into()),
        },
        catalog,
        Arc::new(parking_lot::FairMutex::new(legacy)),
        &config,
        || Ok(("AKIAIOSFODNN7EXAMPLE".into(), "fixture-secret".into())),
        |_| Ok(()),
    )
    .unwrap() else {
        panic!("startup missing");
    };
    let dataset = Arc::new(*dataset);
    // Without a reads table the pilot router is knot-only: no read surface.
    let knot_only = startup::serve(&dataset, None).unwrap();
    let app = startup::serve(&dataset, Some((&reads, admission))).unwrap();
    // Post-cutover: the routed owner, not the legacy writer, holds G's facts.
    let written = crate::auth::run_identified(
        Some(identity("named_bearer")),
        app.clone().oneshot(request(
            "/owner/knot",
            &json!({"graph": GRAPH, "turtle": OWNER}),
        )),
    )
    .await
    .unwrap();
    assert_eq!(written.status(), StatusCode::OK);

    // Pre-cutover baseline: the same facts in G of one legacy single file.
    let mut baseline = Store::open_in_memory().unwrap();
    let graph = baseline.graph_create(GRAPH).unwrap();
    quipu::tool_knot(&mut baseline, &json!({"graph": GRAPH, "turtle": OWNER})).unwrap();
    let text =
        format!("SELECT ?s ?p ?o FROM NAMED <{GRAPH}> WHERE {{ GRAPH <{GRAPH}> {{ ?s ?p ?o }} }}");
    let context = sparql::TemporalContext {
        graph: sparql::GraphScope::Default(vec![graph]),
        named_dataset: Some(vec![graph]),
        ..Default::default()
    };
    let expected = sparql::query_temporal(&baseline, &text, &context).unwrap();
    let (_, expected) = w3c::serialize(&baseline, &expected, w3c::ResultFormat::SparqlJson)
        .unwrap()
        .unwrap();
    let expected = rows(&expected);
    assert_eq!(expected.len(), 4, "baseline positive control");

    let read = |app: axum::Router, token: &'static str| {
        let body = json!({"graphs":[GRAPH], "query": text}).to_string();
        crate::auth::run_identified(
            Some(identity("named_bearer")),
            app.oneshot(
                Request::post("/owner/query")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(body))
                    .unwrap(),
            ),
        )
    };
    let response = read(app.clone(), "routed-read-token").await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    assert_eq!(
        rows(&bytes),
        expected,
        "merged read parity with pre-cutover multiset"
    );
    drop(bytes);
    // Admission stays authenticated: an unregistered bearer is refused.
    assert_eq!(
        read(app.clone(), "not-registered").await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        read(knot_only, "routed-read-token").await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn reads_refuse_a_reservation_outside_the_memory_budget() {
    let (dir, _legacy, catalog, _config, _emulator, _tls) = fixture();
    let mut reads = reads(dir.path());
    reads.query_mib = 1 << 20;
    assert!(startup::read_admission(&catalog, 1024, &reads).is_err());
    reads.query_mib = u64::MAX;
    assert!(startup::read_admission(&catalog, 1024, &reads).is_err());
}

#[tokio::test]
async fn pilot_off_mount_returns_the_router_untouched() {
    // Absent and `enabled = false` read nothing (the catalog path does not
    // exist) and mount nothing: the legacy `/query` is the only one served.
    let (dir, legacy, _catalog, _config, _emulator, _tls) = fixture();
    let shared: crate::SharedStore = crate::StoreHandle::writer_only(legacy).into_shared();
    let mut config = quipu::QuipuConfig::default();
    let base = || axum::Router::new().route("/query", axum::routing::post(|| async { "legacy" }));
    let off = async |config: &quipu::QuipuConfig| {
        let app = startup::mount(base(), config, &shared).with_state(shared.clone());
        let mut seen = Vec::new();
        for path in ["/query", "/owner/query", "/owner/knot"] {
            let response = app
                .clone()
                .oneshot(Request::post(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), 1 << 10)
                .await
                .unwrap();
            seen.push((status, body));
        }
        seen
    };
    let reference = off(&config).await;
    assert_eq!(reference[0], (StatusCode::OK, "legacy".into()));
    assert_eq!(reference[1].0, StatusCode::NOT_FOUND);
    assert_eq!(reference[2].0, StatusCode::NOT_FOUND);
    let mut table: quipu::config::IngestPilotConfig = toml::from_str(
        r#"enabled = false
graph = "urn:g"
root = "/nonexistent"
inventory = "/nonexistent/inventory"
profile = "p"
endpoint = "https://witness.invalid"
bucket = "b"
region = "r"
prefix = "p"
timeout_secs = 5
installed_policy = "p"
credentials_file = "/nonexistent/credentials.json"
catalog_file = "/nonexistent/catalog.json"
memory_mib = 512
"#,
    )
    .unwrap();
    table.reads = Some(reads(dir.path()));
    config.ingest_pilot = Some(table);
    assert_eq!(off(&config).await, reference);
}
