//! `[quipu.graph_sets]` enrolled startup (aegis-n0wc.5): two enrolled set files
//! admitted through `GraphSetsConfig::admit_enrolled` over verified TLS to the
//! conditional-object emulator, then the Phase 1 held-writer gate through the
//! admitted router. No real witness is contacted.
use super::*;
use crate::config::GraphSetsConfig;
use crate::graph_catalog::{Budget, GraphSet, GraphSetCatalog, Route};
use crate::ownership_handoff_model::witness::s3::{Location, S3Objects};
use crate::ownership_handoff_model::witness::s3_emulator::{TIMEOUT, example_credentials};
use crate::store::Datum;
use crate::store::graph_owner::{GraphSetBatch, GraphSetRouter};
use crate::types::{Op, Value as TermValue};
use std::collections::BTreeMap;
use std::sync::mpsc;

const SETS: [(&str, [&str; 2]); 2] = [
    (
        "alpha",
        [
            "https://example.test/alpha/data",
            "https://example.test/alpha/data#provenance",
        ],
    ),
    (
        "beta",
        [
            "https://example.test/beta/data",
            "https://example.test/beta/data#provenance",
        ],
    ),
];

struct Owners {
    _dir: tempfile::TempDir,
    _tls: TlsBridge,
    _emulator: Emulator,
    root: PathBuf,
    routes: Vec<Route>,
    terms: Vec<(i64, i64)>,
    legacy: Store,
    config: GraphSetsConfig,
}

fn authority(route: &Route) -> Authority {
    Authority {
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
    }
}

/// Two set files in one root; `enroll[i]` controls whether file i is enrolled.
fn owners(enroll: [bool; 2]) -> Owners {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let emulator = Emulator::start();
    let tls = TlsBridge::start(&emulator.endpoint);
    let mut routes = Vec::new();
    let mut terms = Vec::new();
    let mut records = Vec::new();
    let mut prefixes = BTreeMap::new();
    for (i, (file, graphs)) in SETS.iter().enumerate() {
        let path = root.join(format!("{file}.db"));
        {
            let store = Store::open(path.to_str().unwrap()).unwrap();
            for graph in graphs {
                store.graph_create(graph).unwrap();
            }
        }
        Store::adopt(path.to_str().unwrap()).unwrap();
        let store = Store::open(path.to_str().unwrap()).unwrap();
        let route = Route {
            graph: graphs[0].into(),
            file_id: (*file).into(),
            store_id: store.store_id().unwrap(),
            log_epoch: store.log_epoch().unwrap(),
            generation: 4,
            policy: "policy-v1".into(),
        };
        terms.push((
            store.intern("urn:item").unwrap(),
            store.intern("urn:value").unwrap(),
        ));
        drop(store);
        let prefix = format!("lineage/{file}");
        prefixes.insert(route.graph.clone(), prefix.clone());
        if enroll[i] {
            let authority = authority(&route);
            let journal = root.join(format!("{file}.journal"));
            ownership_journal::create(
                &journal,
                &Journal::Active {
                    authority: authority.clone(),
                },
            )
            .unwrap();
            fs::set_permissions(&journal, fs::Permissions::from_mode(0o600)).unwrap();
            let meta = fs::metadata(&path).unwrap();
            let entry = ownership_inventory::Entry {
                store_id: route.store_id.clone(),
                unit: AuthorityUnit::FullStore,
                database: ownership_inventory::Database {
                    path: path.to_str().unwrap().into(),
                    device: meta.dev(),
                    inode: meta.ino(),
                },
                journal: journal.to_str().unwrap().into(),
                witness: "sets".into(),
                enrolled_at_generation: 0,
            };
            let binding = ownership_inventory::Binding {
                version: 1,
                store_id: route.store_id.clone(),
                inventory_digest: entry.digest().unwrap(),
            };
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(ownership_admission::MARKER_SQL).unwrap();
            conn.execute(
                "INSERT INTO ownership_handoff_enrollment(id,binding) VALUES(1,?1)",
                [serde_json::to_string(&binding).unwrap()],
            )
            .unwrap();
            let lineage = S3Objects::new(
                Location::new(&emulator.endpoint, BUCKET, "us-east-1", &prefix).unwrap(),
                example_credentials(),
                TIMEOUT,
            );
            assert_eq!(
                lineage.create("active/00", &assumed_genesis_bytes(&authority)),
                Reply::Ok(Created::New)
            );
            records.push(entry);
        }
        routes.push(route);
    }
    let inventory = root.join("inventory");
    fs::write(
        &inventory,
        ownership_inventory::Inventory {
            version: 1,
            records,
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&inventory, fs::Permissions::from_mode(0o600)).unwrap();
    let credentials = root.join("witness.json");
    fs::write(
        &credentials,
        serde_json::json!({
            "access_key_id": "AKIAIOSFODNN7EXAMPLE",
            "secret_access_key": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
        })
        .to_string(),
    )
    .unwrap();
    fs::set_permissions(&credentials, fs::Permissions::from_mode(0o600)).unwrap();
    let ca = root.join("witness-ca.der");
    fs::write(&ca, &tls.der).unwrap();
    fs::set_permissions(&ca, fs::Permissions::from_mode(0o644)).unwrap();
    let legacy = Store::open(root.join("legacy.db").to_str().unwrap()).unwrap();
    for (_, graphs) in SETS {
        for graph in graphs {
            legacy.fence_routed_graph(graph, 7, 4).unwrap();
        }
    }
    let config = GraphSetsConfig {
        enabled: true,
        catalog_file: root.join("unused-graph-sets.json"),
        root: root.clone(),
        installed_policy: "policy-v1".into(),
        memory_mib: 1024,
        inventory,
        profile: "sets".into(),
        endpoint: tls.endpoint.clone(),
        bucket: BUCKET.into(),
        region: "us-east-1".into(),
        timeout_secs: 2,
        witness_ca_file: Some(ca),
        credentials_file: credentials,
        prefixes,
        reads: None,
    };
    Owners {
        _dir: dir,
        _tls: tls,
        _emulator: emulator,
        root,
        routes,
        terms,
        legacy,
        config,
    }
}

fn catalog(o: &Owners) -> Arc<GraphSetCatalog> {
    Arc::new(
        GraphSetCatalog::new(
            7,
            "legacy-store",
            o.routes
                .iter()
                .zip(SETS)
                .map(|(route, (_, graphs))| GraphSet {
                    owner: route.clone(),
                    graphs: graphs.iter().map(|g| (*g).into()).collect(),
                })
                .collect(),
            Budget {
                max_owners: 2,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 2,
                max_connections: 6,
            },
            4,
        )
        .unwrap(),
    )
}

fn requests(set: usize) -> Vec<WriteRequest<'static>> {
    SETS[set]
        .1
        .into_iter()
        .map(|graph| WriteRequest {
            graph: Some(graph),
            catalog_revision: 7,
            owner_generation: Some(4),
            policy: "policy-v1",
            signed: false,
            cross_graph_context: false,
        })
        .collect()
}

fn batch(o: &Owners, set: usize, value: &str) -> GraphSetBatch {
    let (entity, attribute) = o.terms[set];
    let datum = Datum {
        entity,
        attribute,
        value: TermValue::Str(value.into()),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op: Op::Assert,
    };
    GraphSetBatch {
        store_id: o.routes[set].store_id.clone(),
        log_epoch: o.routes[set].log_epoch.clone(),
        graphs: SETS[set]
            .1
            .iter()
            .map(|g| ((*g).to_string(), vec![datum.clone()]))
            .collect(),
        timestamp: "2026-01-01T00:00:00Z".into(),
        actor: Some("startup-test".into()),
        source: Some("startup-test".into()),
    }
}

fn admit(o: &Owners) -> Result<GraphSetRouter> {
    o.config
        .admit_enrolled(catalog(o), &o.legacy, None, |_| Ok(()))
}

type Commit = (
    std::thread::JoinHandle<()>,
    mpsc::Receiver<Result<Vec<(String, i64)>>>,
);

fn commit_in_thread(router: &Arc<GraphSetRouter>, set: usize, batch: GraphSetBatch) -> Commit {
    let (tx, rx) = mpsc::channel();
    let router = router.clone();
    let child = std::thread::spawn(move || {
        let _ = tx.send(router.transact(&requests(set), &batch));
    });
    (child, rx)
}

#[test]
fn enabled_startup_admits_two_enrolled_set_files_and_each_commits_while_the_other_is_held() {
    let o = owners([true, true]);
    let router = Arc::new(admit(&o).expect("enrolled graph-set startup"));
    for (held, free) in [(0, 1), (1, 0)] {
        let round = format!("held-{held}");
        let held_owner = router.owner(SETS[held].1[0]).unwrap();
        let guard = held_owner.hold_writer_for_test();
        let (held_child, held_rx) = commit_in_thread(&router, held, batch(&o, held, &round));
        let (free_child, free_rx) = commit_in_thread(&router, free, batch(&o, free, &round));
        let free_result = free_rx.recv_timeout(Duration::from_secs(3));
        let held_early = held_rx.recv_timeout(Duration::from_millis(300));
        drop(guard);
        free_child.join().unwrap();
        let heads = free_result.expect("free set waited on the held set's writer");
        assert_eq!(heads.unwrap().len(), 2, "free set committed both members");
        assert!(
            held_early.is_err(),
            "held set committed while its writer was held: latch is not a real control"
        );
        let heads = held_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("held set never completed after release");
        held_child.join().unwrap();
        assert_eq!(heads.unwrap().len(), 2);
    }
    // Admitted owners hold their files: no second writer, no legacy facts.
    for (file, _) in SETS {
        assert!(Store::open(o.root.join(format!("{file}.db")).to_str().unwrap()).is_err());
    }
    let legacy_facts: i64 = o
        .legacy
        .conn
        .query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(legacy_facts, 0);
}

#[test]
fn one_unenrolled_set_file_refuses_the_whole_startup_with_no_plain_open_fallback() {
    let o = owners([true, false]);
    let err = admit(&o)
        .err()
        .expect("unenrolled owner admitted")
        .to_string();
    eprintln!("unenrolled refusal: {err}");
    // Refused by S1 at the unenrolled file itself, not merely later because
    // the router found an owner missing.
    assert!(err.contains("store handoff"), "{err}");
    assert!(!err.contains("no prepared owner"), "{err}");
    // The enrolled owner opened first was released with the failed startup:
    // a plain open of its file now refuses only because it is enrolled, never
    // because an owner still holds it.
    let alpha = Store::open(o.root.join("alpha.db").to_str().unwrap())
        .err()
        .expect("an enrolled file opened plain")
        .to_string();
    assert!(alpha.contains("enrolled startup requires"), "{alpha}");
    assert!(!alpha.contains("is active"), "lease survived: {alpha}");
    // The unenrolled file never gained a writer from the refused startup.
    Store::open(o.root.join("beta.db").to_str().unwrap()).expect("unenrolled file claimed");
}

#[test]
fn prefix_and_fence_refusals_come_before_any_credential_is_read() {
    // No credentials file exists, so reaching credentials would name it.
    let mut o = owners([true, true]);
    fs::remove_file(&o.config.credentials_file).unwrap();
    let mut missing = o.config.clone();
    missing.prefixes.pop_first();
    let err = missing
        .admit_enrolled(catalog(&o), &o.legacy, None, |_| Ok(()))
        .err()
        .expect("missing prefix admitted")
        .to_string();
    assert!(err.contains("prefixes"), "{err}");
    let mut extra = o.config.clone();
    extra
        .prefixes
        .insert("https://example.test/unrouted".into(), "lineage/x".into());
    let err = extra
        .admit_enrolled(catalog(&o), &o.legacy, None, |_| Ok(()))
        .err()
        .expect("extra prefix admitted")
        .to_string();
    assert!(err.contains("prefixes"), "{err}");
    // A legacy fence missing one member refuses before credentials too.
    o.legacy = Store::open(o.root.join("legacy-unfenced.db").to_str().unwrap()).unwrap();
    o.legacy.fence_routed_graph(SETS[0].1[0], 7, 4).unwrap();
    let err = admit(&o).err().expect("partial fence admitted").to_string();
    assert!(err.contains("legacy fence"), "{err}");
}

/// aegis-n0wc.5.2 review (#1375 finding 1): with reads the owners take the
/// read admission's own allocation, so the service holds ONE split of
/// `memory_mib`. A missing, foreign or unrequested allocation refuses, and
/// before any credential is read.
#[test]
fn reads_share_the_read_admission_allocation_or_refuse_before_credentials() {
    let o = owners([true, true]);
    fs::remove_file(&o.config.credentials_file).unwrap();
    let mut with_reads = o.config.clone();
    with_reads.reads = Some(crate::config::PilotReadsConfig {
        policy_file: o.root.join("policy.db"),
        owner: "urn:issuer".into(),
        registry_file: o.root.join("registry.json"),
        timeout_secs: 5,
        concurrency: 1,
        runtime_mib: 8,
        view_mib: 8,
        query_mib: 8,
        max_response_mib: 1,
    });
    let readers = 1 + 2;
    let split = |mib, scratch| {
        catalog(&o)
            .physical()
            .memory_allocation(mib, readers, scratch)
            .unwrap()
    };
    let refusal = |config: &GraphSetsConfig, shared| {
        config
            .admit_enrolled(catalog(&o), &o.legacy, shared, |_| Ok(()))
            .err()
            .expect("allocation mismatch admitted")
            .to_string()
    };
    let err = refusal(&with_reads, None);
    assert!(err.contains("no read admission allocation"), "{err}");
    let foreign = split(o.config.memory_mib * 2, 1);
    let err = refusal(&with_reads, Some(foreign));
    assert!(err.contains("differs"), "{err}");
    let err = refusal(&o.config, Some(split(o.config.memory_mib, 0)));
    assert!(err.contains("no reads are configured"), "{err}");
    // The matching split passes the check and reaches the credentials.
    let err = refusal(&with_reads, Some(split(o.config.memory_mib, 1)));
    assert!(!err.contains("allocation"), "{err}");
}
