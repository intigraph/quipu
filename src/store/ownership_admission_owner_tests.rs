//! 4qv: a graph Owner opens an enrolled owner file only through S1.
//! Real enrolled file, protected journal/inventory and witness; no test pin.
use super::*;
use crate::graph_catalog::{Budget, Catalog, Route, WriteRequest};
use crate::store::graph_owner::Owner;
use serde_json::json;
use std::sync::Arc;

const GRAPH: &str = "https://example.test/graphs/enrolled";

/// An owner file is created, given its graph and Stage-A adopted while it is
/// still ordinary; enrollment comes last. Adoption refuses an enrolled file.
fn adopted_fixture() -> Fixture {
    Fixture::with_setup(|db| {
        Store::open(db).unwrap().graph_create(GRAPH).unwrap();
        Store::adopt(db).unwrap();
    })
}

/// Read-only inspection: no admission, so no startup receipt.
fn route(f: &Fixture) -> Route {
    let store = Store::open_read_only(f.db.to_str().unwrap()).unwrap();
    assert!(!f.receipt().exists());
    Route {
        graph: GRAPH.into(),
        file_id: "store".into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation: 4,
        policy: "policy-v1".into(),
    }
}

fn catalog(route: Route) -> Arc<Catalog> {
    Arc::new(
        Catalog::new(
            7,
            "legacy-store",
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
    )
}

fn root(f: &Fixture) -> PathBuf {
    f.db.parent().unwrap().canonicalize().unwrap()
}

fn open(
    f: &Fixture,
    catalog: Arc<Catalog>,
    root: &Path,
    graph: &str,
    policy: &str,
    configure: impl FnOnce(&mut Store) -> Result<()>,
) -> Result<Owner> {
    Owner::open_enrolled(catalog, root, graph, &f.config(), policy, configure, None)
}

fn refused_without_owner(result: Result<Owner>, f: &Fixture, why: &str, before_s1: bool) {
    match result {
        Err(Error::PolicyDenied(msg)) => {
            assert!(!before_s1 || msg.contains("graph owner:"), "{why}: {msg}");
        }
        Err(other) => panic!("{why}: unexpected error {other}"),
        Ok(_) => panic!("{why}: owner opened"),
    }
    assert!(!f.receipt().exists(), "{why}: no startup receipt");
}

#[test]
fn enrolled_owner_opens_through_s1_and_writes_through_the_seam() {
    let f = adopted_fixture();
    let route = route(&f);
    let configured = std::cell::Cell::new(false);
    let owner = open(&f, catalog(route), &root(&f), GRAPH, "policy-v1", |_| {
        configured.set(true);
        Ok(())
    })
    .expect("enrolled owner");
    assert!(configured.get(), "served writer configuration installed");
    assert!(f.receipt().exists(), "S1 receipt for this startup");

    let request = WriteRequest {
        graph: Some(GRAPH),
        catalog_revision: 7,
        owner_generation: Some(4),
        policy: "policy-v1",
        signed: false,
        cross_graph_context: false,
    };
    let input = json!({
        "graph": GRAPH,
        "turtle": "<urn:test:subject> <urn:test:value> \"enrolled\" .",
        "replace_snapshot": true,
        "snapshot": "test-producer",
        "source": "test-producer",
    });
    owner.knot(&request, &input).expect("routed knot");
    let read = owner
        .read(|store| {
            crate::tool_query(
                store,
                &json!({
                    "graph": GRAPH,
                    "query": "SELECT ?v WHERE { <urn:test:subject> <urn:test:value> ?v }"
                }),
            )
        })
        .unwrap();
    assert_eq!(read["count"], 1);

    // The owner holds exclusive admission: a second S1 writer is refused.
    assert!(f.open().is_err(), "second enrolled writer while owned");
    drop(owner);
    f.open().expect("S1 admission after the owner drops");
}

#[test]
fn catalog_refusals_precede_admission_and_write_no_receipt() {
    let f = adopted_fixture();
    let route = route(&f);
    let other = tempfile::tempdir().unwrap();
    let other = other.path().canonicalize().unwrap();
    // The last case names no file under that root: S1 refuses it before any
    // receipt, so it need not be a catalog refusal.
    let root = root(&f);
    let cases: [(&str, &Path, &str, &str, bool); 3] = [
        (
            "unregistered graph",
            &root,
            "https://example.test/graphs/other",
            "policy-v1",
            true,
        ),
        ("policy mismatch", &root, GRAPH, "policy-v2", true),
        ("wrong placement root", &other, GRAPH, "policy-v1", false),
    ];
    for (why, root, graph, policy, before_s1) in cases {
        let result = open(&f, catalog(route.clone()), root, graph, policy, |_| Ok(()));
        refused_without_owner(result, &f, why, before_s1);
    }
    // Positive control: the same fixture still admits with the right inputs,
    // so the refusals above were not an unusable file.
    open(&f, catalog(route), &root, GRAPH, "policy-v1", |_| Ok(())).expect("control");
}

#[test]
fn witness_doubt_returns_no_owner() {
    let f = adopted_fixture();
    let route = route(&f);
    assert_eq!(f.source.witness.inject(Fault::Unknown), Reply::Ok(()));
    match open(
        &f,
        catalog(route),
        &root(&f),
        GRAPH,
        "policy-v1",
        |_| Ok(()),
    ) {
        Err(Error::PolicyDenied(msg)) => {
            assert!(msg.contains("enrolled startup refused"), "{msg}");
        }
        Err(other) => panic!("unexpected error {other}"),
        Ok(_) => panic!("owner opened on witness doubt"),
    }
    assert!(!f.receipt().exists(), "no receipt on refusal");
}

#[test]
fn configuration_failure_returns_no_owner_and_releases_the_lease() {
    let f = adopted_fixture();
    let route = route(&f);
    let result = open(&f, catalog(route), &root(&f), GRAPH, "policy-v1", |_| {
        Err(Error::PolicyDenied("fixture configuration refused".into()))
    });
    assert!(
        matches!(&result, Err(Error::PolicyDenied(m)) if m.contains("fixture configuration")),
        "configuration error is returned"
    );
    drop(result);
    f.open().expect("no leaked enrolled handle or lease");
}

/// S1 lineage admission is not Stage-A native-placement ownership. A file
/// enrolled without prior adoption is admitted by S1 yet reports INACTIVE,
/// and the owner refuses it rather than treating admission as adoption.
#[test]
fn enrolled_but_unadopted_file_is_refused_as_inactive() {
    let f = Fixture::with_setup(|db| {
        Store::open(db).unwrap().graph_create(GRAPH).unwrap();
    });
    let route = route(&f);
    match open(
        &f,
        catalog(route),
        &root(&f),
        GRAPH,
        "policy-v1",
        |_| Ok(()),
    ) {
        Err(Error::PolicyDenied(msg)) => assert!(msg.contains("INACTIVE"), "{msg}"),
        Err(other) => panic!("unexpected error {other}"),
        Ok(_) => panic!("unadopted enrolled file became an owner"),
    }
    f.open().expect("no leaked enrolled handle or lease");
}

/// The read pool shares the writer's one admission, so a journal retirement
/// after startup stops the owner's reads as well as its writes.
#[test]
fn journal_retirement_after_startup_stops_owner_reads() {
    let f = adopted_fixture();
    let route = route(&f);
    let owner = open(
        &f,
        catalog(route),
        &root(&f),
        GRAPH,
        "policy-v1",
        |_| Ok(()),
    )
    .expect("enrolled owner");
    owner.read(Store::store_id).expect("read while admitted");
    ownership_journal::advance(
        &f.journal,
        &Journal::Active {
            authority: authority(),
        },
        &Journal::Retiring {
            request: certificate("op1").request,
        },
    )
    .unwrap();
    assert!(
        owner.read(Store::store_id).is_err(),
        "a retired owner keeps serving reads"
    );
}
