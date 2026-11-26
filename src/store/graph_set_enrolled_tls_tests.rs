//! Graph-set owners through the same S1 enrolled open as the pilot, over real
//! verified TLS to the conditional-object emulator (aegis-n0wc.5).
use super::*;
use crate::graph_catalog::{Budget, GraphSet, GraphSetCatalog, Route};
use crate::store::Datum;
use crate::store::graph_owner::{GraphSetBatch, GraphSetOwner};
use crate::types::{Op, Value as TermValue};

const DATA: &str = "https://example.test/app/data";
const PROV: &str = "https://example.test/app/data#provenance";

struct SetFixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    route: Route,
    /// File-local term IDs interned before the owner opens.
    terms: (i64, i64),
}

fn set_fixture() -> SetFixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("app.db");
    {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        store.graph_create(DATA).unwrap();
        store.graph_create(PROV).unwrap();
    }
    Store::adopt(path.to_str().unwrap()).unwrap();
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let route = Route {
        graph: DATA.into(),
        file_id: "app".into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation: 4,
        policy: "policy-v1".into(),
    };
    let terms = (
        store.intern("urn:item").unwrap(),
        store.intern("urn:value").unwrap(),
    );
    SetFixture {
        _dir: dir,
        root,
        route,
        terms,
    }
}

fn set_catalog(route: &Route, members: &[&str]) -> Arc<GraphSetCatalog> {
    Arc::new(
        GraphSetCatalog::new(
            7,
            "legacy-store",
            vec![GraphSet {
                owner: route.clone(),
                graphs: members.iter().map(|g| (*g).into()).collect(),
            }],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 3,
            },
            4,
        )
        .unwrap(),
    )
}

fn set_config(f: &SetFixture, tls: &TlsBridge) -> EnrolledPilotConfig {
    EnrolledPilotConfig {
        root: f.root.clone(),
        inventory: f.root.join("inventory"),
        profile: "fixture".into(),
        endpoint: tls.endpoint.clone(),
        bucket: BUCKET.into(),
        region: "us-east-1".into(),
        prefix: PREFIX.into(),
        timeout: Duration::from_secs(2),
        witness_ca_der: Some(tls.der.clone()),
        installed_policy: "policy-v1".into(),
        allocation: crate::memory_budget::MemoryAllocation::for_connections(1024, 4).unwrap(),
    }
}

/// Same enrollment shape as the pilot fixture, for the set's owner file.
fn enroll_set(f: &SetFixture, c: &EnrolledPilotConfig, emulator: &Emulator) {
    let authority = Authority {
        store_id: f.route.store_id.clone(),
        unit: AuthorityUnit::FullStore,
        generation: 0,
        epoch: f.route.log_epoch.clone(),
        owner: Placement {
            principal: "fixture-owner".into(),
            native: [1; 32],
        },
        policy: f.route.policy.clone(),
        predecessor: None,
    };
    let journal = f.root.join("owner.journal");
    ownership_journal::create(
        &journal,
        &Journal::Active {
            authority: authority.clone(),
        },
    )
    .unwrap();
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o600)).unwrap();
    let db = f.root.join("app.db");
    let meta = fs::metadata(&db).unwrap();
    let entry = ownership_inventory::Entry {
        store_id: f.route.store_id.clone(),
        unit: AuthorityUnit::FullStore,
        database: ownership_inventory::Database {
            path: db.to_str().unwrap().into(),
            device: meta.dev(),
            inode: meta.ino(),
        },
        journal: journal.to_str().unwrap().into(),
        witness: c.profile.clone(),
        enrolled_at_generation: 0,
    };
    let binding = ownership_inventory::Binding {
        version: 1,
        store_id: f.route.store_id.clone(),
        inventory_digest: entry.digest().unwrap(),
    };
    fs::write(
        &c.inventory,
        ownership_inventory::Inventory {
            version: 1,
            records: vec![entry],
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&c.inventory, fs::Permissions::from_mode(0o600)).unwrap();
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute_batch(ownership_admission::MARKER_SQL).unwrap();
    conn.execute(
        "INSERT INTO ownership_handoff_enrollment(id,binding) VALUES(1,?1)",
        [serde_json::to_string(&binding).unwrap()],
    )
    .unwrap();
    assert_eq!(
        emulator
            .backend()
            .create("active/00", &assumed_genesis_bytes(&authority)),
        Reply::Ok(Created::New)
    );
}

fn credentials() -> Result<(String, String)> {
    Ok((
        "AKIAIOSFODNN7EXAMPLE".into(),
        "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
    ))
}

fn write_requests() -> Vec<WriteRequest<'static>> {
    [DATA, PROV]
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

#[test]
fn enrolled_set_owner_opens_over_verified_tls_commits_every_member_and_refuses_untrusted_tls() {
    let f = set_fixture();
    let emulator = Emulator::start();
    let tls = TlsBridge::start(&emulator.endpoint);
    let mut c = set_config(&f, &tls);
    enroll_set(&f, &c, &emulator);
    let catalog = set_catalog(&f.route, &[DATA, PROV]);

    // Untrusted TLS: refused, no owner.
    let trusted = c.witness_ca_der.take();
    assert!(
        GraphSetOwner::open_enrolled(catalog.clone(), PROV, &c, credentials, |_| Ok(())).is_err()
    );
    c.witness_ca_der = trusted;

    // Any member selects the one physical owner.
    let owner = GraphSetOwner::open_enrolled(catalog.clone(), PROV, &c, credentials, |_| Ok(()))
        .expect("verified TLS S1 graph-set owner");
    let datum = Datum {
        entity: f.terms.0,
        attribute: f.terms.1,
        value: TermValue::Str("present".into()),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op: Op::Assert,
    };
    let batch = GraphSetBatch {
        store_id: f.route.store_id.clone(),
        log_epoch: f.route.log_epoch.clone(),
        graphs: vec![
            (DATA.into(), vec![datum.clone()]),
            (PROV.into(), vec![datum]),
        ],
        timestamp: "2026-01-01T00:00:00Z".into(),
        actor: Some("fixture".into()),
        source: Some("fixture".into()),
    };
    let heads = owner.transact(&write_requests(), &batch).unwrap();
    assert_eq!(heads.len(), 2);
    // While admitted, the file refuses a second writer (no fallback writer).
    assert!(Store::open(f.root.join("app.db").to_str().unwrap()).is_err());
}

#[test]
fn invalid_witness_config_or_policy_refuses_before_credentials_or_open() {
    let f = set_fixture();
    let emulator = Emulator::start();
    let tls = TlsBridge::start(&emulator.endpoint);
    let base = set_config(&f, &tls);
    let catalog = set_catalog(&f.route, &[DATA, PROV]);
    let mut cases = vec![];
    let mut c = base.clone();
    c.profile.clear();
    cases.push(c);
    let mut c = base.clone();
    c.endpoint = "http://witness.example.test".into();
    cases.push(c);
    let mut c = base.clone();
    c.timeout = Duration::ZERO;
    cases.push(c);
    let mut c = base.clone();
    c.installed_policy = "another-policy".into();
    cases.push(c);
    for c in cases {
        assert!(
            GraphSetOwner::open_enrolled(
                catalog.clone(),
                DATA,
                &c,
                || panic!("invalid configuration resolved credentials"),
                |_| panic!("invalid configuration opened owner"),
            )
            .is_err()
        );
    }
    // An unregistered graph refuses before credentials too.
    assert!(
        GraphSetOwner::open_enrolled(
            catalog,
            "https://example.test/unregistered",
            &base,
            || panic!("unregistered graph resolved credentials"),
            |_| panic!("unregistered graph opened owner"),
        )
        .is_err()
    );
}

#[test]
fn an_unenrolled_file_or_a_missing_member_refuses_with_no_plain_open_fallback() {
    // Not enrolled: S1 refuses; nothing opens the file writable instead.
    let f = set_fixture();
    let emulator = Emulator::start();
    let tls = TlsBridge::start(&emulator.endpoint);
    let c = set_config(&f, &tls);
    let catalog = set_catalog(&f.route, &[DATA, PROV]);
    assert!(GraphSetOwner::open_enrolled(catalog, DATA, &c, credentials, |_| Ok(())).is_err());

    // Enrolled, but the catalog names a member the file does not commit.
    let g = set_fixture();
    let emulator = Emulator::start();
    let tls = TlsBridge::start(&emulator.endpoint);
    let c = set_config(&g, &tls);
    enroll_set(&g, &c, &emulator);
    let catalog = set_catalog(&g.route, &[DATA, PROV, "https://example.test/app/absent"]);
    let err = GraphSetOwner::open_enrolled(catalog, DATA, &c, credentials, |_| Ok(()))
        .err()
        .expect("missing member admitted")
        .to_string();
    assert!(err.contains("absent or not committed"), "{err}");
}
