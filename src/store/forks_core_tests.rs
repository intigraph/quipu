//! Fork promote on the merge core (aegis-karb.2, slice S2), checked against
//! `docs/design/merge-behaviour.md` and malcolm's conditions for ruling B:
//! the write gate still binds, a winner's re-assert after a receipt is live,
//! receipts are current at no time, and aegis-0s2a cannot recur.

use rusqlite::params;

use super::*;
use crate::sparql::{QueryResult, TemporalContext, query_temporal};

const T0: &str = "2026-01-01T00:00:00Z";
#[cfg(any(feature = "shacl", feature = "owl"))]
const TO: &str = "2026-01-02T00:00:00Z";
#[cfg(feature = "shacl")]
const TT: &str = "2026-01-03T00:00:00Z";
const TF: &str = "2026-01-04T00:00:00Z";
const TR: &str = "2026-01-05T00:00:00Z";
const TD: &str = "2026-01-10T00:00:00Z";

#[cfg(feature = "shacl")]
const ONE: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
    <urn:t:S> a sh:NodeShape ; sh:targetSubjectsOf <urn:t:status> ;\n\
    sh:property [ sh:path <urn:t:status> ; sh:maxCount 1 ] .\n";

fn d(e: i64, a: i64, v: &str, from: &str, op: Op) -> Datum {
    Datum {
        entity: e,
        attribute: a,
        value: Value::Str(v.into()),
        valid_from: from.into(),
        valid_to: None,
        op,
    }
}

/// Current, or at valid time `at` as known at transaction `tx`.
fn values(store: &Store, at: Option<&str>, tx: Option<i64>) -> Vec<String> {
    let ctx = TemporalContext {
        valid_at: at.map(str::to_string),
        as_of_tx: tx,
        ..Default::default()
    };
    let QueryResult::Select { rows, .. } = query_temporal(
        store,
        "SELECT ?o WHERE { <urn:t:item> <urn:t:status> ?o }",
        &ctx,
    )
    .unwrap() else {
        panic!("expected SELECT");
    };
    let mut out: Vec<String> = rows
        .iter()
        .map(|r| match &r["o"] {
            Value::Str(s) => s.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    out.sort();
    out
}

/// ROOT's assertion rows for the slot: `(value, valid_from, valid_to)`.
fn rows(store: &Store, e: i64, a: i64) -> Vec<(String, String, Option<String>)> {
    let mut st = store
        .conn
        .prepare(
            "SELECT v, valid_from, valid_to FROM facts \
             WHERE g = ?1 AND e = ?2 AND a = ?3 AND op = 1 ORDER BY rowid",
        )
        .unwrap();
    st.query_map(params![crate::schema::ROOT_GRAPH, e, a], |r| {
        Ok((r.get::<_, Vec<u8>>(0)?, r.get(1)?, r.get(2)?))
    })
    .unwrap()
    .map(|r| {
        let (v, f, t) = r.unwrap();
        let Value::Str(s) = Value::from_bytes(&v).unwrap() else {
            panic!("string value");
        };
        (s, f, t)
    })
    .collect()
}

/// R5/R6 at the latest transaction: no reversed row, and no two non-empty
/// intervals of different values overlapping on this single-valued slot.
fn assert_interval_sanity(rows: &[(String, String, Option<String>)]) {
    let epoch = |t: &str| crate::time::epoch_of_rfc3339(t).unwrap();
    let open = |r: &(String, String, Option<String>)| r.2.as_deref().map_or(i64::MAX, epoch);
    for r in rows {
        assert!(open(r) >= epoch(&r.1), "reversed row {r:?}");
    }
    let live: Vec<_> = rows.iter().filter(|r| open(r) > epoch(&r.1)).collect();
    for (i, x) in live.iter().enumerate() {
        for y in &live[i + 1..] {
            let overlap = epoch(&x.1) < open(y) && epoch(&y.1) < open(x);
            assert!(x.0 == y.0 || !overlap, "overlap {x:?} {y:?}");
        }
    }
}

#[cfg(feature = "shacl")]
/// Spec C4: base "base"@T0; ROOT changes it to "ours"@To, the fork to
/// "theirs"@Tt, under sh:maxCount 1.
fn c4_store() -> (Store, i64, i64) {
    let mut store = Store::open_in_memory().unwrap();
    store.load_shapes("one", ONE, T0).unwrap();
    let (e, a) = (
        store.intern("urn:t:item").unwrap(),
        store.intern("urn:t:status").unwrap(),
    );
    store
        .transact(&[d(e, a, "base", T0, Op::Assert)], T0, None, None)
        .unwrap();
    let fork = store
        .fork_create("f", store.latest_tx_id().unwrap(), T0, None)
        .unwrap();
    store
        .transact(
            &[
                d(e, a, "base", TO, Op::Retract),
                d(e, a, "ours", TO, Op::Assert),
            ],
            TO,
            None,
            None,
        )
        .unwrap();
    store
        .transact_to_graph(
            &[
                d(e, a, "base", TT, Op::Retract),
                d(e, a, "theirs", TT, Op::Assert),
            ],
            TT,
            None,
            None,
            fork.g,
        )
        .unwrap();
    (store, e, a)
}

#[cfg(feature = "shacl")]
fn promote(store: &mut Store, e: i64, a: i64, chosen: &str) -> i64 {
    let (_, conflicts) = store.fork_conflicts("f").unwrap();
    assert_eq!(conflicts.len(), 1, "C4 is one maxCount-1 conflict");
    let decided = ForkResolutions::from([((e, a), vec![Value::Str(chosen.into())])]);
    match store
        .fork_promote_resolved("f", &decided, "sha", "rev", TD, None, false)
        .unwrap()
    {
        ForkPromotion::Promoted { tx, .. } => tx,
        other => panic!("expected Promoted, got {other:?}"),
    }
}

#[cfg(feature = "shacl")]
#[test]
fn c4_theirs_starts_where_ours_ends_through_a_receipt() {
    let (mut store, e, a) = c4_store();
    let tx = promote(&mut store, e, a, "theirs");
    let rows = rows(&store, e, a);
    assert_eq!(
        rows,
        vec![
            ("base".into(), T0.into(), Some(TO.into())),
            ("ours".into(), TO.into(), Some(TD.into())),
            ("theirs".into(), TD.into(), Some(TD.into())), // the receipt
            ("theirs".into(), TD.into(), None),            // the live winner
        ]
    );
    assert_interval_sanity(&rows);
    assert_eq!(values(&store, None, None), ["theirs"]);
    assert_eq!(values(&store, Some(TT), None), ["ours"]);
    // Two sequenced transactions; at the receive pin the receipt is current at
    // no valid time, so ours alone holds the slot.
    let receive = tx - 1;
    assert_eq!(values(&store, Some(TD), Some(receive)), ["ours"]);
    let source = |t| store.get_transaction(t).unwrap().unwrap().source.unwrap();
    assert!(
        source(receive).starts_with("fork-promote:f;"),
        "{}",
        source(receive)
    );
    assert!(source(tx).ends_with(";step=decide"), "{}", source(tx));
}

#[cfg(feature = "shacl")]
#[test]
fn c4_ours_keeps_ours_and_leaves_only_a_receipt() {
    let (mut store, e, a) = c4_store();
    promote(&mut store, e, a, "ours");
    let rows = rows(&store, e, a);
    assert!(rows.contains(&("theirs".into(), TD.into(), Some(TD.into()))));
    assert!(rows.contains(&("ours".into(), TO.into(), None)));
    assert_interval_sanity(&rows);
    for at in [None, Some(TT), Some(TD), Some("2026-12-01T00:00:00Z")] {
        assert_eq!(values(&store, at, None), ["ours"], "at {at:?}");
    }
}

#[cfg(feature = "shacl")]
#[test]
fn the_write_gate_still_binds_an_open_second_value() {
    // Sabotage control for ruling B: the receipt passes the maxCount-1 gate
    // only because it is closed. The same value written open is refused.
    let (mut store, e, a) = c4_store();
    let mut receipt = d(e, a, "theirs", TD, Op::Assert);
    receipt.valid_to = Some(TD.into());
    store.transact(&[receipt], TD, None, None).unwrap();
    let open = store.transact(&[d(e, a, "theirs", TD, Op::Assert)], TD, None, None);
    assert!(
        open.is_err(),
        "an open second value must be refused: {open:?}"
    );
}

#[cfg(feature = "owl")]
#[test]
fn a_receipt_does_not_supersede_a_functional_value() {
    let mut store = Store::open_in_memory().unwrap();
    store.owl_config_mut().validate_on_write = true;
    store
        .load_ontology(
            "t",
            "@prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
             <urn:t:status> a owl:DatatypeProperty, owl:FunctionalProperty .\n",
            T0,
        )
        .unwrap();
    store.invalidate_owl_cache();
    let (e, a) = (
        store.intern("urn:t:item").unwrap(),
        store.intern("urn:t:status").unwrap(),
    );
    store
        .transact(&[d(e, a, "ours", TO, Op::Assert)], TO, None, None)
        .unwrap();
    let mut receipt = d(e, a, "theirs", TD, Op::Assert);
    receipt.valid_to = Some(TD.into());
    store.transact(&[receipt], TD, None, None).unwrap();
    assert_eq!(values(&store, None, None), ["ours"]);
    assert!(rows(&store, e, a).contains(&("ours".into(), TO.into(), None)));
}

#[test]
fn aegis_0s2a_a_root_reclaim_after_the_fork_is_closed_at_promotion() {
    // Base a@T0. The fork closes it at Tf; ROOT closes it and re-claims it at
    // Tr > Tf. The fork's close predates ROOT's row, so the row closes at the
    // promotion instant instead of ending before it starts.
    let mut store = Store::open_in_memory().unwrap();
    let (e, a) = (
        store.intern("urn:t:item").unwrap(),
        store.intern("urn:t:status").unwrap(),
    );
    store
        .transact(&[d(e, a, "a", T0, Op::Assert)], T0, None, None)
        .unwrap();
    let fork = store
        .fork_create("f", store.latest_tx_id().unwrap(), T0, None)
        .unwrap();
    store
        .transact_to_graph(&[d(e, a, "a", TF, Op::Retract)], TF, None, None, fork.g)
        .unwrap();
    store
        .transact(&[d(e, a, "a", TR, Op::Retract)], TR, None, None)
        .unwrap();
    store
        .transact(&[d(e, a, "a", TR, Op::Assert)], TR, None, None)
        .unwrap();
    assert!(matches!(
        store.fork_promote("f", TD, None).unwrap(),
        ForkPromotion::Promoted { .. }
    ));
    let rows = rows(&store, e, a);
    assert!(
        rows.contains(&("a".into(), TR.into(), Some(TD.into()))),
        "{rows:?}"
    );
    assert_interval_sanity(&rows);
    assert!(values(&store, None, None).is_empty());
}
