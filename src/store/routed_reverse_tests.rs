use super::*;
use crate::graph_catalog::{Budget, Catalog, Route};
use serde_json::json;

const GRAPH: &str = "https://example.test/graphs/ingest";
const OTHER: &str = "https://example.test/graphs/other";
const TAG: &str = "snapshot:code:repo";
const TS: &str = "2026-11-11T00:00:00Z";
const AT: &str = "2026-11-26T20:00:00Z";
const CODE: &str = "<urn:rev:file> <urn:rev:defines> <urn:rev:symbol> . \
    <urn:rev:file> <urn:rev:path> \"src/lib.rs\" . \
    <urn:rev:file> <urn:rev:note> \"hello\"@en .";
const PLAN: ReversePlan<'static> = ReversePlan {
    graph: GRAPH,
    catalog_revision: 7,
    generation: 4,
};

fn knot(store: &mut Store, input: &serde_json::Value) {
    crate::tool_knot(store, input).unwrap();
}

struct Lab {
    _dir: tempfile::TempDir,
    legacy: String,
    owner: String,
}

/// enroll -> cutover -> `accepted` owner writes, on disposable files.
fn lab(accepted: usize) -> Lab {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let legacy = root.join("legacy.db").to_str().unwrap().to_string();
    let owner = root.join("owner.db").to_str().unwrap().to_string();
    let mut l = Store::open(&legacy).unwrap();
    knot(
        &mut l,
        &json!({"turtle": CODE, "replace_snapshot": true, "snapshot": "code:repo", "source": "t"}),
    );
    let slice = l
        .export_routed_slice(TAG, crate::schema::ROOT_GRAPH_IRI)
        .unwrap();
    let mut o = Store::open(&owner).unwrap();
    o.graph_create(GRAPH).unwrap();
    o.import_routed_slice(&slice, TAG, GRAPH, TS).unwrap();
    l.fence_routed_graph(GRAPH, 7, 4).unwrap();
    l.cut_over_routed_source(TAG, GRAPH, 7, 4, TS).unwrap();
    for i in 0..accepted {
        knot(
            &mut o,
            &json!({"turtle": format!("<urn:rev:w{i}> <urn:rev:n> \"{i}\" ."), "graph": GRAPH}),
        );
    }
    Lab {
        _dir: dir,
        legacy,
        owner,
    }
}

fn owner_facts(lab: &Lab) -> Vec<GraphFact> {
    Store::open_read_only(&lab.owner)
        .unwrap()
        .export_routed_graph(GRAPH)
        .unwrap()
}

fn bytes(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn round_trip_moves_every_accepted_write_back_to_legacy() {
    let lab = lab(3);
    let owner_before = bytes(&lab.owner);
    let lock = lock_owner_file(&lab.owner).unwrap();
    let facts = lock.export_routed_graph(GRAPH).unwrap();
    assert_eq!(facts.len(), 6, "3 moved + 3 accepted");
    let mut legacy = Store::open(&lab.legacy).unwrap();
    let receipt = legacy
        .reverse_routed_graph(&PLAN, &facts, AT, true)
        .unwrap();
    drop(lock);
    assert_eq!(receipt.state, ReverseState::Lifted);
    assert_eq!((receipt.from, receipt.to), ((7, 4), (8, 5)));
    assert_eq!(receipt.witness_state, "active_unretired");
    assert_eq!(receipt.sources[TAG], 3);
    let held = legacy.export_routed_graph(GRAPH).unwrap();
    assert_eq!(
        graph_multiset_sha256(GRAPH, &held),
        receipt.multiset_sha256,
        "legacy holds exactly the owner's graph, tags included"
    );
    assert!(legacy.routed_graphs_fenced().unwrap().is_empty());
    assert!(legacy.routed_source_fence_rows().unwrap().is_empty());
    // The graph is writable in legacy again, and the owner was never written.
    knot(
        &mut legacy,
        &json!({"turtle": "<urn:rev:after> <urn:rev:n> \"x\" .", "graph": GRAPH}),
    );
    assert_eq!(bytes(&lab.owner), owner_before);
    // Re-running a finished reverse reports the identical receipt.
    assert_eq!(legacy.reverse_receipt(&PLAN, &facts).unwrap(), receipt);
}

#[test]
fn dry_run_writes_nothing() {
    let lab = lab(2);
    let before = bytes(&lab.legacy);
    let facts = owner_facts(&lab);
    let legacy = Store::open_read_only(&lab.legacy).unwrap();
    let receipt = legacy.reverse_receipt(&PLAN, &facts).unwrap();
    drop(legacy);
    assert_eq!(receipt.state, ReverseState::Planned);
    assert_eq!(receipt.at, None);
    assert_eq!(bytes(&lab.legacy), before);
}

#[test]
fn a_crash_after_each_step_resumes_to_the_identical_receipt() {
    let straight = {
        let lab = lab(2);
        let facts = owner_facts(&lab);
        let mut legacy = Store::open(&lab.legacy).unwrap();
        legacy
            .reverse_routed_graph(&PLAN, &facts, AT, true)
            .unwrap()
    };
    for crash_after in 1..=3 {
        let lab = lab(2);
        let facts = owner_facts(&lab);
        {
            let mut legacy = Store::open(&lab.legacy).unwrap();
            for _ in 0..crash_after {
                legacy.reverse_step(&PLAN, &facts, AT).unwrap();
            }
        } // the process dies here
        let mut legacy = Store::open(&lab.legacy).unwrap();
        let resumed = legacy
            .reverse_routed_graph(&PLAN, &facts, "2099-01-01T00:00:00Z", true)
            .unwrap();
        let held = legacy.export_routed_graph(GRAPH).unwrap();
        assert_eq!(graph_multiset_sha256(GRAPH, &held), resumed.multiset_sha256);
        // Accepted writes carry wall-clock valid time, so the digest is per lab.
        let expected = ReverseReceipt {
            multiset_sha256: resumed.multiset_sha256.clone(),
            ..straight.clone()
        };
        assert_eq!(resumed, expected, "crash after step {crash_after}");
    }
}

#[test]
fn fence_admits_no_other_writer_while_reconciling() {
    let lab = lab(1);
    let facts = owner_facts(&lab);
    let mut legacy = Store::open(&lab.legacy).unwrap();
    assert_eq!(
        legacy.reverse_step(&PLAN, &facts, AT).unwrap(),
        ReverseState::Reconciling
    );
    assert_eq!(
        legacy.routed_graph_fence_rows().unwrap(),
        vec![(GRAPH.to_string(), 8, 5)]
    );
    let write = crate::tool_knot(
        &mut legacy,
        &json!({"turtle": "<urn:rev:x> <urn:rev:n> \"1\" .", "graph": GRAPH}),
    );
    assert!(write.is_err(), "a Reconciling graph refuses other writers");
    assert_eq!(
        legacy.reverse_step(&PLAN, &facts, AT).unwrap(),
        ReverseState::Proven
    );
    // Proven is still fenced: lifting is a separate step.
    assert_eq!(
        legacy.routed_graphs_fenced().unwrap(),
        vec![GRAPH.to_string()]
    );
}

#[test]
fn stale_unfenced_or_changed_owner_inputs_refuse() {
    let lab = lab(1);
    let facts = owner_facts(&lab);
    let mut legacy = Store::open(&lab.legacy).unwrap();
    let stale = ReversePlan {
        generation: 3,
        ..PLAN
    };
    assert!(
        legacy.reverse_step(&stale, &facts, AT).is_err(),
        "stale (R,G)"
    );
    let unfenced = ReversePlan {
        graph: OTHER,
        ..PLAN
    };
    assert!(legacy.reverse_step(&unfenced, &facts, AT).is_err());
    legacy.reverse_step(&PLAN, &facts, AT).unwrap();
    // The owner's facts changed (tampered slice) since step 2 was journaled.
    let mut tampered = facts.clone();
    tampered[0].fact.valid_from = "1999-01-01T00:00:00Z".into();
    assert!(legacy.reverse_step(&PLAN, &tampered, AT).is_err());
    assert!(legacy.reverse_step(&PLAN, &facts[1..], AT).is_err());
    assert_eq!(
        legacy.reverse_state(&PLAN, &facts).unwrap().0,
        ReverseState::Reconciling,
        "refusals leave the fence Reconciling"
    );
}

#[test]
fn an_open_owner_file_refuses_the_lock() {
    let lab = lab(0);
    let lock = lock_owner_file(&lab.owner).unwrap();
    assert!(
        lock_owner_file(&lab.owner).is_err(),
        "a second holder refuses"
    );
    assert!(
        Store::open_read_only(&lab.owner)
            .and_then(|s| s.export_routed_graph(GRAPH))
            .is_err(),
        "no second connection can read past the lock"
    );
    drop(lock);
    let mut writer = Store::open(&lab.owner).unwrap();
    assert!(
        lock_owner_file(&lab.owner).is_err(),
        "an idle open owner Store refuses"
    );
    writer.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(
        lock_owner_file(&lab.owner).is_err(),
        "a writing owner refuses"
    );
    writer.conn.execute_batch("ROLLBACK").unwrap();
    knot(
        &mut writer,
        &json!({"turtle": "<urn:rev:a> <urn:rev:n> \"1\" .", "graph": GRAPH}),
    );
    drop(writer);
    let owner_before = bytes(&lab.owner);
    drop(lock_owner_file(&lab.owner).expect("a closed owner admits the lock"));
    assert_eq!(bytes(&lab.owner), owner_before, "the lock writes nothing");
}

const HOLDER_ENV: &str = "QUIPU_REVERSE_IDLE_OWNER_DB";

/// Child side: hold the owner file open in WAL mode, IDLE (no transaction,
/// raw SQLite, no quipu writer lease), until the parent creates `<db>.stop`.
#[test]
#[ignore = "run only as the child process of a_second_process_holding_the_owner_idle_refuses"]
fn idle_owner_holder_child() {
    let Ok(db) = std::env::var(HOLDER_ENV) else {
        return;
    };
    let conn = rusqlite::Connection::open(&db).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let _: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
        .unwrap();
    assert!(conn.is_autocommit(), "the holder stays idle");
    std::fs::write(format!("{db}.ready"), b"ready").unwrap();
    let stop = std::path::PathBuf::from(format!("{db}.stop"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !stop.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_second_process_holding_the_owner_idle_refuses() {
    use std::time::{Duration, Instant};
    let lab = lab(1);
    let ready = std::path::PathBuf::from(format!("{}.ready", lab.owner));
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "store::routed_reverse::tests::idle_owner_holder_child",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(HOLDER_ENV, &lab.owner)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "holder child never became ready");
        assert!(
            child.try_wait().unwrap().is_none(),
            "holder child exited early"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let refused = lock_owner_file(&lab.owner);
    std::fs::write(format!("{}.stop", lab.owner), b"stop").unwrap();
    assert!(child.wait().unwrap().success(), "holder child failed");
    let Err(err) = refused else {
        panic!("an idle second-process holder must refuse the lock");
    };
    assert!(err.to_string().contains("in use"), "{err}");
    let lock = lock_owner_file(&lab.owner).expect("after the holder closes, the lock succeeds");
    assert_eq!(lock.export_routed_graph(GRAPH).unwrap().len(), 4);
}

fn route(graph: &str, file_id: &str, generation: u64) -> Route {
    Route {
        graph: graph.into(),
        file_id: file_id.into(),
        store_id: format!("store-{file_id}"),
        log_epoch: "epoch".into(),
        generation,
        policy: "policy-v1".into(),
    }
}

fn catalog(revision: u64, routes: Vec<Route>) -> Catalog {
    let budget = Budget {
        max_owners: 2,
        queued_per_owner: 1,
        readers_per_owner: 1,
        max_queued: 2,
        max_connections: 6,
    };
    Catalog::new(revision, "legacy-store", routes, budget).unwrap()
}

#[test]
fn catalog_r_plus_one_must_change_only_this_route() {
    let current = catalog(7, vec![route(GRAPH, "ingest", 4), route(OTHER, "other", 2)]);
    let good = catalog(8, vec![route(OTHER, "other", 2)]);
    validate_reverse_catalogs(&current, &good, &PLAN).unwrap();
    let still_routed = catalog(8, vec![route(GRAPH, "ingest", 5), route(OTHER, "other", 2)]);
    assert!(validate_reverse_catalogs(&current, &still_routed, &PLAN).is_err());
    let other_changed = catalog(8, vec![route(OTHER, "other", 3)]);
    assert!(validate_reverse_catalogs(&current, &other_changed, &PLAN).is_err());
    let skipped = catalog(9, vec![route(OTHER, "other", 2)]);
    assert!(validate_reverse_catalogs(&current, &skipped, &PLAN).is_err());
    let wrong_generation = ReversePlan {
        generation: 5,
        ..PLAN
    };
    assert!(validate_reverse_catalogs(&current, &good, &wrong_generation).is_err());
}
