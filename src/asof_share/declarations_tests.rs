//! As-of share carriage of declarations and the outward scrub
//! (write-provenance-persistence §5.2, §6; slice P5).
//!
//! Exit proof: an outward cut carries no `host`/`session` key and its manifest
//! says so; an internal cut carries them; an outward artifact carrying
//! `session` is refused on import with nothing written.

use serde_json::{Value as Json, json};

use super::{Cut, Limits, Request, cut};
use crate::applier::{Applier, Outcome, live_digest};
use crate::share_scrub::ShareDestination;
use crate::store::{Datum, Store};
use crate::types::{Op, Value};

const G: &str = "http://example.org/graph/declared";
const TS: &str = "2026-04-01T00:00:00Z";

fn source() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let g = store.graph_create(G).unwrap();
    let e = store.intern("http://example.org/thing").unwrap();
    let a = store.intern("http://example.org/name").unwrap();
    let datum = Datum {
        entity: e,
        attribute: a,
        value: Value::Str("one".into()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    let tx = store
        .transact_to_graph(&[datum], TS, Some("writer"), Some("test"), g)
        .unwrap();
    let declared = json!({
        "agent": "agent-a", "harness": "harness-h", "host": "box-1",
        "session": "session-1", "model": "model-m"
    });
    store
        .conn
        .execute(
            "INSERT INTO transaction_declarations (tx, declarations) VALUES (?1, ?2)",
            rusqlite::params![tx, declared.to_string()],
        )
        .unwrap();
    store
}

fn take(store: &Store, destination: ShareDestination, dir: &std::path::Path) -> Cut {
    let request = Request {
        graphs: vec![G.into()],
        context_graphs: Vec::new(),
        tx: None,
        expected_store_id: store.store_id().unwrap(),
        destination,
    };
    cut(store, &request, dir, &Limits::default()).unwrap_or_else(|e| panic!("{e:?}"))
}

fn records(cut: &Cut) -> Vec<Json> {
    std::fs::read_to_string(&cut.spool)
        .unwrap()
        .lines()
        .flat_map(|line| {
            let part: Json = serde_json::from_str(line).unwrap();
            part["records"].as_array().unwrap().clone()
        })
        .collect()
}

fn import(manifest: &Json, records: &[Json]) -> (Store, Outcome) {
    let mut replica = Store::open_in_memory().unwrap();
    let mut applier = Applier::open(&replica).unwrap();
    let frame = json!({"manifest": manifest, "records": records}).to_string();
    let outcome = applier
        .apply_frame(&mut replica, "quipu.asof", &frame, 1)
        .unwrap();
    (replica, outcome)
}

#[test]
fn an_outward_cut_scrubs_host_and_session_and_says_so() {
    let store = source();
    let dir = tempfile::tempdir().unwrap();
    let cut = take(&store, ShareDestination::Outward, dir.path());
    assert_eq!(cut.manifest.declarations_scrubbed, ["host", "session"]);
    let manifest = serde_json::to_value(&cut.manifest).unwrap();
    assert_eq!(
        manifest["declarations_scrubbed"],
        json!(["host", "session"])
    );
    assert!(
        manifest.get("destination").is_none(),
        "outward is the default"
    );
    let records = records(&cut);
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0]["declared"],
        json!({"agent": "agent-a", "harness": "harness-h", "model": "model-m"})
    );
    let spool = std::fs::read_to_string(&cut.spool).unwrap();
    for leaked in ["box-1", "session-1", "\"host\"", "\"session\""] {
        assert!(!spool.contains(leaked), "{leaked} left the store: {spool}");
    }
    // The scrubbed cut still imports, digest and all.
    let (replica, outcome) = import(&manifest, &records);
    assert!(
        matches!(outcome, Outcome::Bootstrapped { .. }),
        "{outcome:?}"
    );
    assert_eq!(live_digest(&replica), live_digest(&store));
}

#[test]
fn an_internal_cut_carries_every_declared_key() {
    let store = source();
    let dir = tempfile::tempdir().unwrap();
    let cut = take(&store, ShareDestination::Internal, dir.path());
    assert!(cut.manifest.declarations_scrubbed.is_empty());
    let manifest = serde_json::to_value(&cut.manifest).unwrap();
    assert_eq!(manifest["destination"], json!("internal"));
    assert!(manifest.get("declarations_scrubbed").is_none());
    let records = records(&cut);
    assert_eq!(records[0]["declared"]["host"], json!("box-1"));
    assert_eq!(records[0]["declared"]["session"], json!("session-1"));
    let (_, outcome) = import(&manifest, &records);
    assert!(
        matches!(outcome, Outcome::Bootstrapped { .. }),
        "{outcome:?}"
    );
}

#[test]
fn an_outward_artifact_carrying_session_is_refused_on_import() {
    let store = source();
    let dir = tempfile::tempdir().unwrap();
    // An internal cut relabelled outward: the bytes a producer that skipped
    // the scrub would send. The receiver refuses before writing anything.
    let cut = take(&store, ShareDestination::Internal, dir.path());
    let mut manifest = serde_json::to_value(&cut.manifest).unwrap();
    manifest.as_object_mut().unwrap().remove("destination");
    let (replica, outcome) = import(&manifest, &records(&cut));
    match outcome {
        Outcome::Refused { reason } => {
            assert!(reason.contains("operational declaration"), "{reason}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(live_digest(&replica).is_empty(), "nothing written");
}
