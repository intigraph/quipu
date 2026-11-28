//! H1 matrix (aegis-n0wc.5 phase 4): the Seeds item write path through
//! `/update`'s real dispatch with an enrolled graph-set router.
use super::*;
use crate::{SharedStore, StoreHandle};
use quipu::{
    Store,
    graph_catalog::{Budget, GraphSet, GraphSetCatalog, Route as CatalogRoute},
    store::graph_owner::{GraphSetOwner, GraphSetRouter},
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const P: &str = "https://example.test/project/p";
const PROV: &str = "https://example.test/project/p#seeds-provenance";
const EPH: &str = "https://example.test/project/p#seeds-ephemeral";
const Q: &str = "https://example.test/project/q";
const LEG: &str = "https://example.test/legacy/l";
const BASE: &str = "BASE <http://localhost/update>\n";

struct Fx {
    _dir: tempfile::TempDir,
    root: PathBuf,
    legacy: SharedStore,
    router: GraphSetRouter,
    tracker_id: String,
}

fn adopted(root: &Path, file: &str, graphs: &[&str]) -> Store {
    let path = root.join(file);
    {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        for g in graphs {
            store.graph_create(g).unwrap();
        }
    }
    Store::adopt(path.to_str().unwrap()).unwrap();
    Store::open(path.to_str().unwrap()).unwrap()
}

fn route(graph: &str, file: &str, store: &Store) -> CatalogRoute {
    CatalogRoute {
        graph: graph.into(),
        file_id: file.into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation: 4,
        policy: "policy-v1".into(),
    }
}

/// Set A = {P, PROV, EPH} in tracker.db, set B = {Q} in other.db, LEG legacy.
fn fx() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let tracker = adopted(&root, "tracker.db", &[P, PROV, EPH]);
    let other = adopted(&root, "other.db", &[Q]);
    let legacy = Store::open(root.join("legacy.db").to_str().unwrap()).unwrap();
    // Pre-cutover copies stay in legacy, so only the fence refuses a bypass.
    for g in [P, PROV, EPH, Q, LEG] {
        legacy.graph_create(g).unwrap();
    }
    let (a, b) = (route(P, "tracker", &tracker), route(Q, "other", &other));
    let tracker_id = a.store_id.clone();
    let catalog = Arc::new(
        GraphSetCatalog::new(
            7,
            &legacy.store_id().unwrap(),
            vec![
                GraphSet {
                    owner: a,
                    graphs: vec![P.into(), PROV.into(), EPH.into()],
                },
                GraphSet {
                    owner: b,
                    graphs: vec![Q.into()],
                },
            ],
            Budget {
                max_owners: 2,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 2,
                max_connections: 8,
            },
            8,
        )
        .unwrap(),
    );
    for g in [P, PROV, EPH, Q] {
        legacy.fence_routed_graph(g, 7, 4).unwrap();
    }
    let allocation = || catalog.physical().memory_allocation(512, 1, 1).unwrap();
    let owners = vec![
        GraphSetOwner::prepare(
            catalog.clone(),
            &root,
            P,
            tracker,
            "policy-v1",
            allocation(),
        )
        .unwrap(),
        GraphSetOwner::prepare(catalog.clone(), &root, Q, other, "policy-v1", allocation())
            .unwrap(),
    ];
    let router = GraphSetRouter::admit(catalog, &legacy, owners).unwrap();
    Fx {
        _dir: dir,
        root,
        legacy: Arc::new(StoreHandle::writer_only(legacy)),
        router,
        tracker_id,
    }
}

fn seeds() -> super::super::Attribution {
    super::super::Attribution {
        actor: Some("seeds".into()),
        source: "seeds".into(),
    }
}

fn run(router: Option<&GraphSetRouter>, legacy: &SharedStore, update: &str) -> Dispatched {
    dispatch(router, legacy, &format!("{BASE}{update}"), &seeds(), "test").unwrap()
}

fn report(d: Dispatched) -> serde_json::Value {
    match d {
        Dispatched::Report(r) => r,
        Dispatched::Refused(m) => panic!("unexpected refusal: {m}"),
        Dispatched::Retried(s) => panic!("unexpected retry status: {s}"),
    }
}

fn refusal(d: Dispatched) -> String {
    match d {
        Dispatched::Refused(m) => m,
        Dispatched::Report(r) | Dispatched::Retried(r) => {
            panic!("expected a 409 refusal, got {r}")
        }
    }
}

/// (current facts, highest tx) of one file, read straight from SQLite.
fn head(root: &Path, file: &str) -> (i64, i64) {
    let c = rusqlite::Connection::open_with_flags(
        root.join(file),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let facts = c
        .query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    let tx = c
        .query_row("SELECT coalesce(max(id), 0) FROM transactions", [], |r| {
            r.get(0)
        })
        .unwrap();
    (facts, tx)
}

/// Whether `file` holds an asserted `<s> <p> ?o` fact in graph `g`.
fn holds(root: &Path, file: &str, g: &str, s: &str, p: &str) -> bool {
    let c = rusqlite::Connection::open_with_flags(
        root.join(file),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    c.query_row(
        "SELECT count(*) FROM facts f \
         JOIN terms ge ON ge.id = f.g JOIN terms se ON se.id = f.e JOIN terms pe ON pe.id = f.a \
         WHERE ge.iri = ?1 AND se.iri = ?2 AND pe.iri = ?3",
        [g, s, p],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        > 0
}

const CREATE: &str = "INSERT DATA { \
    GRAPH <https://example.test/project/p> { <urn:i> <urn:rev> 1 ; <urn:title> \"t\" } \
    GRAPH <https://example.test/project/p#seeds-provenance> { <urn:w1> <urn:of> <urn:i> } }";

/// The Seeds edit shape: revision CAS in WHERE, provenance in the same request.
fn edit(from: u32, to: u32, write: &str) -> String {
    format!(
        "DELETE {{ GRAPH <{P}> {{ <urn:i> <urn:rev> ?r }} }} \
         INSERT {{ GRAPH <{P}> {{ <urn:i> <urn:rev> {to} ; <urn:status> \"closed\" }} \
                  GRAPH <{PROV}> {{ <{write}> <urn:of> <urn:i> }} }} \
         WHERE {{ GRAPH <{P}> {{ <urn:i> <urn:rev> ?r FILTER(?r = {from}) }} \
                  FILTER NOT EXISTS {{ GRAPH <{EPH}> {{ <urn:i> <urn:held> ?h }} }} }}"
    )
}

#[test]
fn m1_create_and_cas_edit_with_provenance_commit_in_the_owner_file_only() {
    let f = fx();
    let legacy_before = head(&f.root, "legacy.db");
    let created = report(run(Some(&f.router), &f.legacy, CREATE));
    assert_eq!(created["asserted"], 3);
    assert_eq!(created["owner"]["store_id"], f.tracker_id.as_str());
    let local_tx = created["owner"]["local_tx"].as_i64().unwrap();
    assert_eq!(head(&f.root, "tracker.db").1, local_tx);
    let edited = report(run(Some(&f.router), &f.legacy, &edit(1, 2, "urn:w2")));
    assert_eq!(
        (edited["asserted"].as_u64(), edited["retracted"].as_u64()),
        (Some(3), Some(1))
    );
    assert!(edited["owner"]["local_tx"].as_i64().unwrap() > local_tx);
    assert!(holds(&f.root, "tracker.db", P, "urn:i", "urn:status"));
    assert!(holds(&f.root, "tracker.db", PROV, "urn:w2", "urn:of"));
    let actor: Option<String> = rusqlite::Connection::open(f.root.join("tracker.db"))
        .unwrap()
        .query_row(
            "SELECT actor FROM transactions WHERE id = ?1",
            [edited["owner"]["local_tx"].as_i64().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(actor.as_deref(), Some("seeds"));
    assert_eq!(
        head(&f.root, "legacy.db"),
        legacy_before,
        "legacy file unchanged"
    );
}

#[test]
fn m2_a_stale_revision_changes_nothing_and_leaves_the_owner_head() {
    let f = fx();
    report(run(Some(&f.router), &f.legacy, CREATE));
    report(run(Some(&f.router), &f.legacy, &edit(1, 2, "urn:w2")));
    let before = head(&f.root, "tracker.db");
    let lost = report(run(Some(&f.router), &f.legacy, &edit(1, 3, "urn:w3")));
    assert_eq!(
        (lost["asserted"].as_u64(), lost["retracted"].as_u64()),
        (Some(0), Some(0))
    );
    assert!(lost["tx"].is_null() && lost["owner"]["local_tx"].is_null());
    assert_eq!(head(&f.root, "tracker.db"), before);
}

/// R3 refusals: nothing is evaluated, and BOTH files keep zero new facts and
/// an unchanged head after the 409.
fn assert_refused_with_no_write(f: &Fx, update: &str) {
    let files = ["tracker.db", "other.db", "legacy.db"];
    let before: Vec<_> = files.iter().map(|x| head(&f.root, x)).collect();
    let message = refusal(run(Some(&f.router), &f.legacy, update));
    assert!(
        message.starts_with("update spans writable files"),
        "{message}"
    );
    let after: Vec<_> = files.iter().map(|x| head(&f.root, x)).collect();
    assert_eq!(after, before, "{update}");
}

#[test]
fn m3_an_update_spanning_an_owner_and_another_file_is_refused_before_any_write() {
    let f = fx();
    report(run(Some(&f.router), &f.legacy, CREATE));
    for update in [
        format!(
            "INSERT DATA {{ GRAPH <{P}> {{ <urn:i> <urn:x> 1 }} GRAPH <{LEG}> {{ <urn:i> <urn:x> 1 }} }}"
        ),
        format!(
            "INSERT DATA {{ GRAPH <{P}> {{ <urn:i> <urn:x> 1 }} GRAPH <{Q}> {{ <urn:i> <urn:x> 1 }} }}"
        ),
        format!(
            "INSERT {{ GRAPH <{P}> {{ <urn:i> <urn:x> 1 }} }} WHERE {{ GRAPH <{LEG}> {{ ?s ?p ?o }} }}"
        ),
    ] {
        assert_refused_with_no_write(&f, &update);
    }
}

#[test]
fn m4_variable_graph_default_graph_and_using_reach_with_a_member_are_refused() {
    let f = fx();
    report(run(Some(&f.router), &f.legacy, CREATE));
    for update in [
        format!(
            "INSERT {{ GRAPH <{P}> {{ <urn:i> <urn:x> 1 }} }} WHERE {{ GRAPH ?g {{ ?s ?p ?o }} }}"
        ),
        format!("INSERT {{ GRAPH <{P}> {{ <urn:i> <urn:x> 1 }} }} WHERE {{ ?s ?p ?o }}"),
        format!("INSERT DATA {{ GRAPH <{P}> {{ <urn:i> <urn:x> 1 }} <urn:i> <urn:x> 1 }}"),
        format!(
            "INSERT {{ GRAPH <{P}> {{ <urn:i> <urn:x> 1 }} }} USING <{P}> WHERE {{ ?s ?p ?o }}"
        ),
        format!("CLEAR GRAPH <{P}>"),
        format!("DROP GRAPH <{P}>"),
        // Open reach refuses even with no routed graph named: GRAPH ?g would
        // read the members' stale legacy copies.
        format!(
            "INSERT {{ GRAPH <{LEG}> {{ <urn:s> <urn:q> 1 }} }} WHERE {{ GRAPH ?g {{ ?s ?p ?o }} }}"
        ),
        "CLEAR ALL".to_owned(),
    ] {
        assert_refused_with_no_write(&f, &update);
    }
}

#[test]
fn m5_an_update_on_no_enrolled_graph_is_identical_to_the_unrouted_path() {
    let (routed, plain) = (fx(), fx());
    for update in [
        format!("INSERT DATA {{ GRAPH <{LEG}> {{ <urn:s> <urn:p> 1 }} <urn:s> <urn:p> 2 }}"),
        format!(
            "DELETE {{ GRAPH <{LEG}> {{ ?s <urn:p> ?o }} }} INSERT {{ GRAPH <{LEG}> {{ ?s <urn:p> 3 }} }} WHERE {{ GRAPH <{LEG}> {{ ?s <urn:p> ?o }} }}"
        ),
    ] {
        let a = report(run(Some(&routed.router), &routed.legacy, &update));
        let b = report(run(None, &plain.legacy, &update));
        assert_eq!(a, b, "{update}");
        assert!(a.get("owner").is_none());
    }
    assert_eq!(
        head(&routed.root, "legacy.db"),
        head(&plain.root, "legacy.db")
    );
    assert_eq!(
        head(&routed.root, "tracker.db"),
        head(&plain.root, "tracker.db")
    );
}

#[test]
fn m6_a_dropped_response_is_found_by_reading_back_data_and_provenance() {
    let f = fx();
    drop(run(Some(&f.router), &f.legacy, CREATE));
    // The Seeds read-back: the data and its provenance marker, no replay journal.
    assert!(holds(&f.root, "tracker.db", P, "urn:i", "urn:rev"));
    assert!(holds(&f.root, "tracker.db", PROV, "urn:w1", "urn:of"));
    // A blind retry of the create is a no-op, not a second write.
    let before = head(&f.root, "tracker.db");
    let again = report(run(Some(&f.router), &f.legacy, CREATE));
    assert_eq!(again["asserted"], 0);
    assert_eq!(head(&f.root, "tracker.db"), before);
}

#[test]
fn m7_the_seeds_owner_commits_while_another_owner_and_legacy_are_held() {
    let f = fx();
    let hold = |file: &str| {
        let c = rusqlite::Connection::open(f.root.join(file)).unwrap();
        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        c
    };
    let (_other, _legacy) = (hold("other.db"), hold("legacy.db"));
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            done.send(run(Some(&f.router), &f.legacy, CREATE)).unwrap();
        });
        let r = report(
            finished
                .recv_timeout(std::time::Duration::from_secs(20))
                .expect("Seeds owner waited on another file's writer"),
        );
        assert_eq!(r["asserted"], 3);
    });
}

/// Sabotage row: an R3 update forced onto the legacy path must not commit
/// anything there either. The legacy fence refuses the routed member, so a
/// router that sent R3 to legacy is an error, never a partial write.
#[test]
fn sabotage_an_r3_update_sent_to_legacy_writes_nothing_in_any_file() {
    let f = fx();
    let files = ["tracker.db", "other.db", "legacy.db"];
    let before: Vec<_> = files.iter().map(|x| head(&f.root, x)).collect();
    let update = format!(
        "{BASE}INSERT DATA {{ GRAPH <{P}> {{ <urn:i> <urn:x> 1 }} GRAPH <{LEG}> {{ <urn:i> <urn:x> 1 }} }}"
    );
    assert!(super::super::apply_update_reported(&f.legacy, &update, &seeds(), "test").is_err());
    let after: Vec<_> = files.iter().map(|x| head(&f.root, x)).collect();
    assert_eq!(after, before);
}

/// Seconds held on `/update` by exactly `client`, read from the rendered
/// exposition; `None` when that client has no series (never recorded, or
/// folded into the overflow bucket by other tests in this process).
fn update_held(client: &str) -> Option<f64> {
    let label = format!("client=\"{client}\"");
    let series: Vec<f64> = quipu::metrics::metrics()
        .render_with_graph_counts(None, None)
        .lines()
        .filter(|line| line.starts_with("quipu_store_held_seconds_total{"))
        .filter(|line| line.contains("endpoint=\"/update\"") && line.contains(&label))
        .filter_map(|line| line.rsplit(' ').next()?.parse::<f64>().ok())
        .collect();
    (!series.is_empty()).then(|| series.iter().sum())
}

/// H2a: a routed write records store time for its client like a legacy write
/// does. Before, `apply_routed` built its phase timings and dropped them.
/// Only this client's own series counts: the shared overflow bucket can be
/// raised by other tests, so it would let a dropped observation pass.
#[test]
fn h2a_a_routed_update_records_store_time_for_its_client() {
    let f = fx();
    let before = update_held("h2a-routed").unwrap_or(0.0);
    let created = dispatch(
        Some(&f.router),
        &f.legacy,
        &format!("{BASE}{CREATE}"),
        &seeds(),
        "h2a-routed",
    )
    .unwrap();
    assert_eq!(report(created)["owner"]["store_id"], f.tracker_id.as_str());
    let after = update_held("h2a-routed").expect(
        "no h2a-routed /update series: the routed write recorded nothing, or the \
         client was folded and this test cannot judge",
    );
    assert!(
        after > before,
        "the routed write held the owner's writer and must say so"
    );
}

/// H2a: the owner reports its hold on success, and no hold on a refusal made
/// before its writer was taken (a refused write still reports its wait).
#[test]
fn h2a_the_owner_reports_held_only_when_it_held_its_writer() {
    let f = fx();
    let Ok(Route::Owner(owner, members)) =
        super::route(Some(&f.router), &format!("{BASE}{CREATE}"))
    else {
        panic!("CREATE routes to the tracker owner");
    };
    let (refused, hold) = owner.update_observed(&Default::default(), |_| Ok(()));
    assert!(refused.is_err(), "an empty selection is refused");
    assert_eq!(hold.held, None, "refused before the writer was held");
    let (done, hold) = owner.update_observed(&members, |_| Ok(()));
    assert!(done.is_ok());
    assert!(hold.held.is_some(), "the writer was held for the work");
}

/// H2b: a committed routed receipt carries a strict cut over its members, at
/// or after the write it reports.
#[test]
fn h2b_a_committed_receipt_carries_a_cut_at_or_after_its_write() {
    use quipu::store::composition_cut::CompositionCut;
    let f = fx();
    let created = report(run(Some(&f.router), &f.legacy, CREATE));
    let local_tx = created["owner"]["local_tx"].as_i64().unwrap();
    let token = created["owner"]["cut"].as_str().expect("a cut token");
    let cut = CompositionCut::from_token(token).unwrap();
    assert!(!cut.members().is_empty());
    for (graph, member) in cut.members() {
        assert_eq!(
            member.store_id, f.tracker_id,
            "{graph} is in the owner file"
        );
        assert!(
            member.local_tx >= local_tx,
            "{graph} is at or after the write"
        );
    }
    let edited = report(run(Some(&f.router), &f.legacy, &edit(1, 2, "urn:w2")));
    let later = CompositionCut::from_token(edited["owner"]["cut"].as_str().unwrap()).unwrap();
    assert_ne!(later, cut, "a second write moves the cut");
}

/// H2b: a CAS that committed nothing has no write to cut at.
#[test]
fn h2b_a_write_that_committed_nothing_has_no_cut() {
    let f = fx();
    report(run(Some(&f.router), &f.legacy, CREATE));
    let lost = report(run(Some(&f.router), &f.legacy, &edit(7, 8, "urn:w9")));
    assert!(lost["owner"]["local_tx"].is_null());
    assert!(lost["owner"].get("cut").is_none());
}

/// H2b: a refused capture never turns a committed write into an error. The
/// receipt keeps the commit and says why there is no cut.
#[test]
fn h2b_a_refused_capture_leaves_the_committed_receipt_with_a_null_cut() {
    let f = fx();
    let Ok(Route::Owner(owner, _)) = super::route(Some(&f.router), &format!("{BASE}{CREATE}"))
    else {
        panic!("CREATE routes to the tracker owner");
    };
    let mut receipt = serde_json::json!({"owner": {"local_tx": 7}});
    // A graph this owner does not hold makes the capture refuse.
    let foreign: BTreeSet<String> = [Q.to_string()].into();
    attach_cut(owner, &foreign, &mut receipt);
    assert_eq!(receipt["owner"]["local_tx"], 7, "the commit is kept");
    assert!(receipt["owner"]["cut"].is_null());
    assert!(
        receipt["owner"]["cut_unavailable"]
            .as_str()
            .is_some_and(|why| why.contains("not a graph of this owner")),
        "{receipt}"
    );
}

// ---- H3 (aegis-n0wc.5): durable write IDs on routed `/update` ----

use quipu::store::knot_completion::update::{UpdateAdmission, UpdateTicket};
use quipu::transaction_auth::{Identity, with_identity};

fn who(name: &str) -> Option<Identity> {
    Some(Identity {
        principal: format!("urn:crew:{name}"),
        credential_id: Some(name.into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    })
}

fn wid(c: char) -> String {
    c.to_string().repeat(64)
}

/// One `/update` dispatch as `name`, carrying `write_id`.
fn durable(f: &Fx, name: &str, write_id: Option<&str>, update: &str) -> Dispatched {
    with_identity(who(name), || {
        dispatch_with_write_id(
            Some(&f.router),
            &f.legacy,
            &format!("{BASE}{update}"),
            &seeds(),
            "test",
            write_id,
        )
    })
    .unwrap()
}

fn retried_status(d: Dispatched) -> serde_json::Value {
    match d {
        Dispatched::Retried(s) => s,
        Dispatched::Report(r) => panic!("expected a retry status, got a fresh report {r}"),
        Dispatched::Refused(m) => panic!("expected a retry status, got refusal {m}"),
    }
}

/// The HTTP status a retried write ID answers with.
fn http(status: &serde_json::Value) -> axum::http::StatusCode {
    super::super::write_id::retried(status.clone()).status()
}

/// Admit `write_id` for CREATE as `name` without running it (a crash before
/// the effect), returning the owner's ticket.
fn admit_only(f: &Fx, name: &str, write_id: &str) -> UpdateTicket {
    let text = format!("{BASE}{CREATE}");
    let Ok(Route::Owner(owner, _)) = super::route(Some(&f.router), &text) else {
        panic!("CREATE routes to the tracker owner");
    };
    with_identity(who(name), || {
        match owner.admit_update(update_binding(write_id, &text).unwrap()) {
            Ok(UpdateAdmission::Fresh(ticket)) => ticket,
            other => panic!("expected a fresh admission, got {other:?}"),
        }
    })
}

/// Run CREATE under an already-admitted ticket, never recording its finish.
fn effect_only(f: &Fx, name: &str, ticket: &UpdateTicket) -> Result<serde_json::Value, AppError> {
    let text = format!("{BASE}{CREATE}");
    let Ok(Route::Owner(owner, members)) = super::route(Some(&f.router), &text) else {
        panic!("CREATE routes to the tracker owner");
    };
    with_identity(who(name), || {
        apply_routed_ticketed(owner, &members, &text, &seeds(), "test", Some(ticket))
    })
}

fn ledger_rows(root: &Path) -> Option<i64> {
    rusqlite::Connection::open(root.join("tracker.db"))
        .unwrap()
        .query_row("SELECT count(*) FROM update_attempts", [], |r| r.get(0))
        .ok()
}

/// R1: the same ID and bytes return the original receipt; the data changed
/// once, with exactly the transactions of the same update without an ID.
#[test]
fn r1_a_repeated_write_id_returns_the_original_receipt_and_writes_once() {
    let f = fx();
    let (id, before) = (wid('a'), head(&f.root, "tracker.db"));
    let first = report(durable(&f, "alice", Some(&id), CREATE));
    assert_eq!(first["asserted"], 3);
    let after = head(&f.root, "tracker.db");
    // The same update without a write ID: a write ID adds no transaction
    // (one per changed graph, here P and PROV; the witness rides the last).
    let control = fx();
    let control_before = head(&control.root, "tracker.db").1;
    report(durable(&control, "alice", None, CREATE));
    let control_txs = head(&control.root, "tracker.db").1 - control_before;
    assert_eq!(
        after.1 - before.1,
        control_txs,
        "same transactions as without an ID"
    );
    let again = retried_status(durable(&f, "alice", Some(&id), CREATE));
    assert_eq!(again["state"], "finished");
    assert_eq!(http(&again), axum::http::StatusCode::OK);
    let receipt = &again["receipt"];
    for key in ["asserted", "retracted", "tx"] {
        assert_eq!(receipt[key], first[key], "{key}");
    }
    for key in ["store_id", "local_tx", "cut"] {
        assert_eq!(receipt["owner"][key], first["owner"][key], "owner.{key}");
    }
    assert_eq!(again["commit"]["tx_id"], first["owner"]["local_tx"]);
    assert_eq!(
        head(&f.root, "tracker.db"),
        after,
        "the retry wrote nothing"
    );
    assert_eq!(ledger_rows(&f.root), Some(1));
}

/// R2 and R7: the same ID with other bytes, or the same bytes from another
/// principal, is ONE identical refusal with no receipt and no write.
#[test]
fn r2_r7_a_reused_write_id_with_other_bytes_or_principal_is_one_refusal() {
    let f = fx();
    let id = wid('b');
    let first = report(durable(&f, "alice", Some(&id), CREATE));
    let before = head(&f.root, "tracker.db");
    let other_bytes = refusal(durable(&f, "alice", Some(&id), &edit(1, 2, "urn:w2")));
    let other_principal = refusal(durable(&f, "mallory", Some(&id), CREATE));
    assert!(
        other_bytes.contains("write ID already used"),
        "{other_bytes}"
    );
    assert_eq!(other_bytes, other_principal, "the refusal reveals nothing");
    assert!(!other_principal.contains(first["owner"]["store_id"].as_str().unwrap()));
    assert_eq!(head(&f.root, "tracker.db"), before);
    assert!(!holds(&f.root, "tracker.db", P, "urn:i", "urn:status"));
}

/// R3: an ID admitted whose effect never ran is UNKNOWN custody: a retry
/// answers 409 `admitted` and never runs the update.
#[test]
fn r3_an_admitted_id_with_no_effect_retries_as_409_admitted() {
    let f = fx();
    let id = wid('c');
    admit_only(&f, "alice", &id);
    let before = head(&f.root, "tracker.db");
    let status = retried_status(durable(&f, "alice", Some(&id), CREATE));
    assert_eq!(status["state"], "admitted");
    assert!(status.get("commit").is_none() && status.get("receipt").is_none());
    assert_eq!(http(&status), axum::http::StatusCode::CONFLICT);
    assert_eq!(head(&f.root, "tracker.db"), before, "never re-run");
    assert!(!holds(&f.root, "tracker.db", P, "urn:i", "urn:rev"));
}

/// R4: the commit witness rides the update's own commit, so an effect whose
/// receipt was never recorded retries as 200 `committed` with its tx.
#[test]
fn r4_a_committed_effect_without_its_receipt_retries_as_committed() {
    let f = fx();
    let id = wid('d');
    let ticket = admit_only(&f, "alice", &id);
    let done = effect_only(&f, "alice", &ticket).unwrap();
    let local_tx = done["owner"]["local_tx"].as_i64().unwrap();
    let before = head(&f.root, "tracker.db");
    let status = retried_status(durable(&f, "alice", Some(&id), CREATE));
    assert_eq!(status["state"], "committed");
    assert_eq!(status["commit"]["tx_id"], local_tx);
    assert_eq!(http(&status), axum::http::StatusCode::OK);
    assert_eq!(head(&f.root, "tracker.db"), before);
}

/// R5: without the header nothing changes: no ledger, the ordinary report;
/// and a write ID on a legacy update refuses rather than claim durability.
#[test]
fn r5_no_write_id_is_the_unchanged_path_and_legacy_refuses_one() {
    let f = fx();
    let created = report(durable(&f, "alice", None, CREATE));
    assert_eq!(created["asserted"], 3);
    assert_eq!(ledger_rows(&f.root), None, "no update_attempts table");
    let legacy = format!("INSERT DATA {{ GRAPH <{LEG}> {{ <urn:s> <urn:p> 1 }} }}");
    let before = head(&f.root, "legacy.db");
    let refused = with_identity(who("alice"), || {
        dispatch_with_write_id(
            Some(&f.router),
            &f.legacy,
            &format!("{BASE}{legacy}"),
            &seeds(),
            "test",
            Some(&wid('e')),
        )
    });
    assert!(refused.is_err(), "legacy has no ledger");
    assert_eq!(head(&f.root, "legacy.db"), before);
    report(durable(&f, "alice", None, &legacy));
}

/// R6: admission is bounded; at capacity a fresh ID refuses before any
/// effect while an admitted ID still retries.
#[test]
fn r6_a_full_ledger_refuses_a_fresh_id_before_the_effect() {
    let f = fx();
    let first = wid('f');
    report(durable(
        &f,
        "alice",
        Some(&first),
        &format!("INSERT DATA {{ GRAPH <{P}> {{ <urn:z> <urn:p> 1 }} }}"),
    ));
    rusqlite::Connection::open(f.root.join("tracker.db"))
        .unwrap()
        .execute_batch(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 65535) \
             INSERT INTO update_attempts(write_id,owner,body,store_id,epoch,nonce,phase) \
             SELECT printf('%064x', i),'x','x','x','x','x','sealed' FROM n",
        )
        .unwrap();
    let before = head(&f.root, "tracker.db");
    let full = with_identity(who("alice"), || {
        dispatch_with_write_id(
            Some(&f.router),
            &f.legacy,
            &format!("{BASE}{CREATE}"),
            &seeds(),
            "test",
            Some(&wid('e')),
        )
    });
    assert!(full.is_err(), "capacity refuses");
    assert_eq!(head(&f.root, "tracker.db"), before);
    let again = retried_status(durable(
        &f,
        "alice",
        Some(&first),
        &format!("INSERT DATA {{ GRAPH <{P}> {{ <urn:z> <urn:p> 1 }} }}"),
    ));
    assert_eq!(again["state"], "finished");
}

/// R8: a sealed UNKNOWN attempt never commits, even from a late worker that
/// still holds its ticket; a NEW ID may then run the intent once.
#[test]
fn r8_a_sealed_id_never_runs_and_a_new_id_may_retry_the_intent() {
    let f = fx();
    let (old, new) = (wid('1'), wid('2'));
    let ticket = admit_only(&f, "alice", &old);
    let text = format!("{BASE}{CREATE}");
    let Ok(Route::Owner(owner, _)) = super::route(Some(&f.router), &text) else {
        panic!("CREATE routes to the tracker owner");
    };
    let sealed = with_identity(who("alice"), || {
        owner.seal_update(&update_binding(&old, &text).unwrap())
    })
    .unwrap();
    assert_eq!(sealed["state"], "sealed");
    let before = head(&f.root, "tracker.db");
    assert!(
        effect_only(&f, "alice", &ticket).is_err(),
        "the late worker's write rolls back"
    );
    assert_eq!(head(&f.root, "tracker.db"), before, "nothing committed");
    let status = retried_status(durable(&f, "alice", Some(&old), CREATE));
    assert_eq!(status["state"], "sealed");
    assert_eq!(http(&status), axum::http::StatusCode::CONFLICT);
    let fresh = report(durable(&f, "alice", Some(&new), CREATE));
    assert_eq!(fresh["asserted"], 3, "the intent ran exactly once");
    // A committed attempt cannot be sealed.
    assert!(
        with_identity(who("alice"), || owner
            .seal_update(&update_binding(&new, &text).unwrap()))
        .is_err()
    );
}
