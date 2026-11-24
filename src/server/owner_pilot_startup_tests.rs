//! Combined real TLS/S1 startup, owner HTTP and strict composed snapshots.
use super::*;
const ID: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
fn identity(class: &str) -> quipu::transaction_auth::Identity {
    quipu::transaction_auth::Identity {
        principal: "urn:test:startup".into(),
        credential_id: Some("startup-key".into()),
        auth_class: class.into(),
        attestation: Default::default(),
    }
}
use axum::{body::Body, http::Request};
use quipu::{
    graph_catalog::{Budget, Route},
    ownership_handoff_model::{
        Authority, AuthorityUnit, Journal, Payload, Placement, RECORD_VERSION, Record,
    },
    store::graph_router::{EnrolledPilotConfig, IngestPilot},
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    time::Duration,
};
use tower::ServiceExt;
#[path = "owner_pilot_s3_fixture.rs"]
mod s3;
#[path = "owner_pilot_tls_fixture.rs"]
mod tls;
const GRAPH: &str = "https://example.test/graphs/startup";

fn protected(path: &Path, body: &[u8]) {
    let mut bytes = body.to_vec();
    bytes.extend_from_slice(format!("\nsha256:{}\n", hex::encode(Sha256::digest(body))).as_bytes());
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn fixture() -> (
    tempfile::TempDir,
    Store,
    Arc<Catalog>,
    EnrolledPilotConfig,
    s3::Emulator,
    tls::TlsBridge,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("ingest.db");
    Store::open(path.to_str().unwrap())
        .unwrap()
        .graph_create(GRAPH)
        .unwrap();
    Store::adopt(path.to_str().unwrap()).unwrap();
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let route = Route {
        graph: GRAPH.into(),
        file_id: "ingest".into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation: 4,
        policy: "policy-v1".into(),
    };
    drop(store);
    let legacy_path = root.join("legacy.db");
    drop(Store::open(legacy_path.to_str().unwrap()).unwrap());
    Store::adopt(legacy_path.to_str().unwrap()).unwrap();
    let mut legacy = Store::open(legacy_path.to_str().unwrap()).unwrap();
    quipu::tool_knot(
        &mut legacy,
        &json!({"turtle":"<urn:shared> <urn:root> \"root\" .", "source":"startup-root"}),
    )
    .unwrap();
    let catalog = Arc::new(
        Catalog::new(
            7,
            &legacy.store_id().unwrap(),
            vec![route.clone()],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 3,
            },
        )
        .unwrap()
        .with_legacy_participant(Route {
            graph: quipu::schema::ROOT_GRAPH_IRI.into(),
            file_id: "legacy".into(),
            store_id: legacy.store_id().unwrap(),
            log_epoch: legacy.log_epoch().unwrap(),
            generation: 1,
            policy: "legacy-policy".into(),
        })
        .unwrap(),
    );
    legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    let emulator = s3::Emulator::start();
    let tls = tls::TlsBridge::start(&emulator.endpoint);
    let config = EnrolledPilotConfig {
        root: root.clone(),
        inventory: root.join("inventory"),
        profile: "fixture".into(),
        endpoint: tls.endpoint.clone(),
        bucket: s3::BUCKET.into(),
        region: "us-east-1".into(),
        prefix: s3::PREFIX.into(),
        timeout: Duration::from_secs(2),
        witness_ca_der: Some(tls.der.clone()),
        installed_policy: "policy-v1".into(),
        allocation: catalog.memory_allocation(1024, 3, 0).unwrap(),
    };
    let authority = Authority {
        store_id: route.store_id.clone(),
        unit: AuthorityUnit::FullStore,
        generation: 0,
        epoch: route.log_epoch.clone(),
        owner: Placement {
            principal: "fixture-owner".into(),
            native: [1; 32],
        },
        policy: route.policy.clone(),
        predecessor: None,
    };
    let journal = root.join("owner.journal");
    protected(
        &journal,
        &Record {
            version: RECORD_VERSION,
            payload: Payload::Journal {
                journal: Journal::Active {
                    authority: authority.clone(),
                },
            },
        }
        .encode()
        .unwrap(),
    );
    // Exact wire field order is checked by the normal inventory reader.
    #[derive(Serialize)]
    struct Database {
        path: String,
        device: u64,
        inode: u64,
    }
    #[derive(Serialize)]
    struct Entry {
        store_id: String,
        unit: AuthorityUnit,
        database: Database,
        journal: String,
        witness: String,
        enrolled_at_generation: u64,
    }
    #[derive(Serialize)]
    struct Inventory {
        version: u32,
        records: Vec<Entry>,
    }
    let meta = fs::metadata(&path).unwrap();
    let entry = Entry {
        store_id: route.store_id.clone(),
        unit: AuthorityUnit::FullStore,
        database: Database {
            path: path.to_str().unwrap().into(),
            device: meta.dev(),
            inode: meta.ino(),
        },
        journal: journal.to_str().unwrap().into(),
        witness: config.profile.clone(),
        enrolled_at_generation: 0,
    };
    let digest = hex::encode(Sha256::digest(serde_json::to_vec(&entry).unwrap()));
    protected(
        &config.inventory,
        &serde_json::to_vec(&Inventory {
            version: 1,
            records: vec![entry],
        })
        .unwrap(),
    );
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("CREATE TABLE ownership_handoff_enrollment(id INTEGER PRIMARY KEY CHECK (id = 1), binding TEXT NOT NULL)").unwrap();
    conn.execute(
        "INSERT INTO ownership_handoff_enrollment VALUES(1,?1)",
        [json!({"version":1,"store_id":route.store_id,"inventory_digest":digest}).to_string()],
    )
    .unwrap();
    #[derive(Serialize)]
    #[serde(tag = "kind", content = "body")]
    enum Active {
        Active(Authority),
    }
    #[derive(Serialize)]
    struct Envelope {
        version: u8,
        entry: Active,
    }
    let genesis = serde_json::to_vec(&Envelope {
        version: 1,
        entry: Active::Active(authority),
    })
    .unwrap();
    emulator
        .state
        .lock()
        .unwrap()
        .objects
        .insert(format!("{}/active/00", s3::PREFIX), (genesis, 1));
    (dir, legacy, catalog, config, emulator, tls)
}

fn request(path: &str, input: &Value) -> Request<Body> {
    Request::post(path)
        .header("content-type", "application/json")
        .header("x-quipu-completion-contract", crate::knot_durable::CONTRACT)
        .header("x-quipu-write-id", ID)
        .header("x-quipu-catalog-revision", "7")
        .header("x-quipu-owner-generation", "4")
        .header("x-quipu-owner-policy", "policy-v1")
        .body(Body::from(input.to_string()))
        .unwrap()
}

#[tokio::test]
async fn combined_factory_http_and_composed_snapshot_keep_both_graphs() {
    let (_dir, legacy, catalog, config, _emulator, _tls) = fixture();
    let pilot = IngestPilot {
        enabled: true,
        graph: Some(GRAPH.into()),
    };
    let Startup::Enabled(dataset) = PreparedDataset::initialize(
        &pilot,
        catalog,
        legacy,
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
        panic!("enabled startup disappeared")
    };
    let input = json!({"graph":GRAPH,"turtle":"<urn:shared> <urn:owner> \"owner\" .", "source":"startup-owner"});
    let sqlite = rusqlite::Connection::open(config.root.join("legacy.db")).unwrap();
    sqlite.execute_batch("BEGIN IMMEDIATE").unwrap();
    let app = dataset.routes();
    let response = crate::auth::run_identified(
        Some(identity("named_bearer")),
        app.clone().oneshot(request("/knot", &input)),
    )
    .await
    .unwrap();
    assert!(response.status().is_success(), "{}", response.status());
    let response = crate::auth::run_identified(
        Some(identity("named_bearer")),
        app.oneshot(request(
            "/knot/status",
            &json!({"write_id":ID,"request":input}),
        )),
    )
    .await
    .unwrap();
    assert!(response.status().is_success());
    let proof: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(proof["state"], "finished");
    sqlite.execute_batch("ROLLBACK").unwrap();
    assert!(
        dataset
            .read(
                &[GRAPH],
                Instant::now() + Duration::from_secs(5),
                |_| -> quipu::Result<()> {
                    Err(quipu::Error::PolicyDenied("callback refused".into()))
                }
            )
            .is_err()
    );
    let captured = dataset
        .read(
            &[quipu::schema::ROOT_GRAPH_IRI, GRAPH],
            Instant::now() + Duration::from_secs(5),
            |reads| {
                assert_eq!(reads.len(), 2);
                assert!(reads.iter().all(|r| r.catalog_revision == 7));
                assert_ne!(reads[0].route.store_id, reads[1].route.store_id);
                let mut matches = 0;
                for read in reads {
                    let query = if read.route.graph == GRAPH {
                        format!("SELECT ?s WHERE {{ GRAPH <{GRAPH}> {{ ?s <urn:owner> ?v }} }}")
                    } else {
                        "SELECT ?s WHERE { ?s <urn:root> ?v }".into()
                    };
                    let result = quipu::sparql::query(read.store, &query)?;
                    assert_eq!(result.rows().len(), 1);
                    let expected = quipu::Value::Ref(
                        read.store.lookup("urn:shared")?.expect("shared subject"),
                    );
                    assert_eq!(result.rows()[0].get("s"), Some(&expected));
                    matches += 1;
                }
                Ok(matches)
            },
        )
        .unwrap();
    assert_eq!(captured, 2);
    for graphs in [vec![GRAPH, GRAPH], vec!["urn:unknown"], vec![]] {
        assert!(
            dataset
                .read(
                    &graphs,
                    Instant::now() + Duration::from_secs(5),
                    |_| -> quipu::Result<()> { panic!("invalid selection evaluated") }
                )
                .is_err()
        );
    }
    assert!(
        dataset
            .read(&[GRAPH], Instant::now(), |_| -> quipu::Result<()> {
                panic!("expired selection evaluated")
            })
            .is_err()
    );
}

#[test]
fn disabled_combined_startup_returns_same_legacy_without_callbacks() {
    let (_dir, legacy, catalog, mut config, _emulator, _tls) = fixture();
    let id = legacy.store_id().unwrap();
    config.endpoint.clear();
    let Startup::Disabled(legacy) = PreparedDataset::initialize(
        &IngestPilot::default(),
        catalog,
        legacy,
        &config,
        || panic!("OFF credentials"),
        |_| panic!("OFF configure"),
    )
    .unwrap() else {
        panic!("OFF admitted")
    };
    assert_eq!(legacy.store_id().unwrap(), id);
}

#[test]
fn combined_startup_refuses_incomplete_memory_before_credentials() {
    let (_dir, legacy, catalog, mut config, _emulator, _tls) = fixture();
    config.allocation = quipu::memory_budget::MemoryAllocation::for_connections(1024, 3).unwrap();
    assert!(
        PreparedDataset::initialize(
            &IngestPilot {
                enabled: true,
                graph: Some(GRAPH.into())
            },
            catalog,
            legacy,
            &config,
            || panic!("incomplete memory resolved credentials"),
            |_| panic!("incomplete memory configured owner")
        )
        .is_err()
    );
}

#[test]
fn combined_startup_refuses_untrusted_witness_and_returns_no_http_routes() {
    let (_dir, legacy, catalog, mut config, _emulator, _tls) = fixture();
    config.witness_ca_der = None;
    assert!(
        PreparedDataset::initialize(
            &IngestPilot {
                enabled: true,
                graph: Some(GRAPH.into())
            },
            catalog,
            legacy,
            &config,
            || Ok((
                "AKIAIOSFODNN7EXAMPLE".into(),
                "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into()
            )),
            |_| panic!("untrusted witness configured owner")
        )
        .is_err()
    );
}

#[test]
fn enrolled_startup_composes_authorized_root_owner_join_with_one_memory_budget() {
    use quipu::read_authorization::{
        ComposedReadAdmission, ComposedReadReservation, OwnerReadPolicy, ReadGrant, ReadIdentity,
        ReadOperation,
    };
    let (_dir, legacy, catalog, mut config, _emulator, _tls) = fixture();
    let admission = ComposedReadAdmission::new(
        catalog.clone(),
        1024,
        3,
        1,
        ComposedReadReservation {
            runtime_bytes: 16 << 20,
            view_bytes: 16 << 20,
            query_bytes: 16 << 20,
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
        legacy,
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
        panic!("enabled startup disappeared")
    };
    let mut policy = OwnerReadPolicy::open(":memory:", "urn:principal:owner").unwrap();
    let grant = ReadGrant {
        version: 1,
        id: "urn:grant:startup".into(),
        principal: "urn:test:startup".into(),
        graphs: [quipu::schema::ROOT_GRAPH_IRI.into(), GRAPH.into()].into(),
        operations: [ReadOperation::Query].into(),
        valid_from: 1,
        valid_until: 100,
        max_offline_seconds: 30,
        issuer: "urn:principal:owner".into(),
        revision: 1,
        revoked: false,
    };
    policy
        .apply(&grant, "private startup composition fixture")
        .unwrap();
    let who = ReadIdentity::from_verified_request(&identity("named_bearer"), 100).unwrap();
    let policy = policy.snapshot().unwrap();
    // This owner write happens after all read pools were opened. A fresh strict
    // capture must include it and resolve IRIs through the composed dictionary.
    let input = json!({"graph":GRAPH,"turtle":"<urn:shared> <urn:owner> \"owner\"@en .", "source":"startup-join"});
    // Reuse the admitted HTTP write path rather than opening a second writer.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let response = runtime
        .block_on(crate::auth::run_identified(
            Some(identity("named_bearer")),
            dataset.routes().oneshot(request("/knot", &input)),
        ))
        .unwrap();
    assert!(response.status().is_success());
    let deadline = Instant::now() + Duration::from_secs(5);
    let permit = admission.try_reserve().unwrap();
    let output = dataset.read(&[quipu::schema::ROOT_GRAPH_IRI, GRAPH], deadline, |reads| {
        let view = policy.authorize(&who, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, permit, 2, deadline)?;
        assert!(admission.try_reserve().is_err(), "same scratch reservation must stay held");
        let query = format!("SELECT ?s ?root ?owner FROM <{}> FROM NAMED <{GRAPH}> WHERE {{ ?s <urn:root> ?root . GRAPH <{GRAPH}> {{ ?s <urn:owner> ?owner }} }}", quipu::schema::ROOT_GRAPH_IRI);
        let output = view.query(&query, false, 2)?;
        let value: Value = serde_json::from_slice(output.as_bytes()).unwrap();
        let rows = value["results"]["bindings"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "neither participant alone can answer this join");
        assert_eq!(rows[0]["s"]["value"], "urn:shared");
        assert_eq!(rows[0]["root"]["value"], "root");
        assert_eq!(rows[0]["owner"]["value"], "owner");
        assert_eq!(rows[0]["owner"]["xml:lang"], "en");
        assert!(view.query("ASK { GRAPH <urn:excluded> { ?s ?p ?o } }", false, 2).is_err());
        view.recheck(&policy, &who, reads, 2)?;
        Ok(output)
    }).unwrap();
    assert!(!output.as_bytes().is_empty());
    // Returning the buffered output releases scratch, but retains its own query
    // lease; the caller must not extract unaccounted owned response copies.
    assert!(admission.try_reserve().is_ok());
}
