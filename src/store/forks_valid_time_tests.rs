//! Promotion honors each fork write's valid time exactly like a direct ROOT
//! write (aegis-jxxd): a back-dated correction promoted from a fork closes
//! ROOT's replaced value at the correction's time, not at the promotion
//! instant; transaction time is the promotion transaction.

use super::*;
use crate::sparql::{QueryResult, TemporalContext, query_temporal};

const SEED: &str = "2026-01-01T00:00:00Z";
const CORRECTED: &str = "2026-03-01T00:00:00Z";
const PROMOTED: &str = "2026-06-01T00:00:00Z";

fn d(e: i64, a: i64, v: &str, valid_from: &str, op: Op) -> Datum {
    Datum {
        entity: e,
        attribute: a,
        value: Value::Str(v.into()),
        valid_from: valid_from.into(),
        valid_to: None,
        op,
    }
}

fn names(store: &Store, valid_at: Option<&str>, as_of_tx: Option<i64>) -> Vec<String> {
    let ctx = TemporalContext {
        valid_at: valid_at.map(str::to_string),
        as_of_tx,
        ..Default::default()
    };
    let r = query_temporal(
        store,
        "SELECT ?o WHERE { <urn:t:alice> <urn:t:name> ?o }",
        &ctx,
    )
    .unwrap();
    let QueryResult::Select { rows, .. } = r else {
        panic!("expected SELECT");
    };
    let mut out: Vec<String> = rows
        .iter()
        .flat_map(|row| row.values().map(|v| format!("{v:?}")))
        .map(|s| if s.contains("new") { "new" } else { "old" }.to_string())
        .collect();
    out.sort();
    out
}

/// A store seeded with alice's name "old", valid since `SEED`.
fn seeded() -> (Store, i64, i64) {
    let mut store = Store::open_in_memory().unwrap();
    let alice = store.intern("urn:t:alice").unwrap();
    let name = store.intern("urn:t:name").unwrap();
    store
        .transact(&[d(alice, name, "old", SEED, Op::Assert)], SEED, None, None)
        .unwrap();
    (store, alice, name)
}

/// The correction as a direct write makes it: retract the old value and
/// assert the new one, both at the correction's time.
fn correction(alice: i64, name: i64) -> Vec<Datum> {
    vec![
        d(alice, name, "old", SEED, Op::Retract),
        d(alice, name, "new", CORRECTED, Op::Assert),
    ]
}

#[test]
fn a_back_dated_correction_promoted_from_a_fork_supersedes_at_its_valid_time() {
    let (mut store, alice, name) = seeded();
    let latest = store.latest_tx_id().unwrap();
    let fork = store.fork_create("fix", latest, SEED, None).unwrap();
    store
        .transact_to_graph(&correction(alice, name), CORRECTED, None, None, fork.g)
        .unwrap();
    let before_promote = store.latest_tx_id().unwrap();
    let ForkPromotion::Promoted { tx, .. } = store.fork_promote("fix", PROMOTED, None).unwrap()
    else {
        panic!("expected Promoted");
    };

    // Valid time: old before the correction, new only after it — including
    // the window between the correction and the promotion instant.
    assert_eq!(names(&store, Some("2026-02-01T00:00:00Z"), None), ["old"]);
    assert_eq!(names(&store, Some("2026-04-01T00:00:00Z"), None), ["new"]);
    assert_eq!(names(&store, Some("2026-07-01T00:00:00Z"), None), ["new"]);
    assert_eq!(names(&store, None, None), ["new"]);
    // Transaction time is the promotion: before it, ROOT still said old.
    assert_eq!(
        names(&store, Some("2026-04-01T00:00:00Z"), Some(before_promote)),
        ["old"]
    );
    assert_eq!(
        names(&store, Some("2026-04-01T00:00:00Z"), Some(tx)),
        ["new"]
    );
}

#[test]
fn promoted_and_direct_corrections_answer_every_valid_time_alike() {
    let (mut direct, alice, name) = seeded();
    direct
        .transact(&correction(alice, name), CORRECTED, None, None)
        .unwrap();

    let (mut promoted, alice, name) = seeded();
    let latest = promoted.latest_tx_id().unwrap();
    let fork = promoted.fork_create("fix", latest, SEED, None).unwrap();
    promoted
        .transact_to_graph(&correction(alice, name), CORRECTED, None, None, fork.g)
        .unwrap();
    promoted.fork_promote("fix", PROMOTED, None).unwrap();

    for at in [
        "2025-12-01T00:00:00Z",
        "2026-02-01T00:00:00Z",
        CORRECTED,
        "2026-04-01T00:00:00Z",
        "2026-07-01T00:00:00Z",
    ] {
        assert_eq!(
            names(&promoted, Some(at), None),
            names(&direct, Some(at), None),
            "valid time {at}"
        );
    }
}
