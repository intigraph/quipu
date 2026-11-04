//! A selected datum must not widen a materialization-window retraction.
use super::*;
use crate::{Op, store::Datum};

const TS: &str = "2026-11-04T00:00:00Z";

fn datum(store: &mut Store, value: &str) -> Datum {
    Datum {
        entity: store.intern("urn:window:subject").unwrap(),
        attribute: store.intern("urn:window:value").unwrap(),
        value: Value::Str(value.into()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

fn apply(
    store: &mut Store,
    datum: &Datum,
    source: &str,
    graph: i64,
    window: ClaimWindow,
) -> Result<i64> {
    let mut retract = datum.clone();
    retract.op = Op::Retract;
    store.transact_to_graph_window_checked(
        &[retract],
        TS,
        None,
        Some("owl:materialize"),
        graph,
        Some(source),
        &[],
        None,
        Some(window),
        None,
    )
}

fn live(store: &Store, graph: i64, tx: i64) -> i64 {
    store
        .conn
        .query_row(
            "SELECT count(*) FROM facts WHERE g=?1 AND tx=?2 AND op=1 AND valid_to IS NULL",
            params![graph, tx],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn claim_window_preserves_same_source_outside_bounds_and_other_producers() {
    let mut store = Store::open_in_memory().unwrap();
    let d = datum(&mut store, "shared");
    let graph = store.ensure_companion_inferred_graph(0, TS).unwrap();
    let early = store
        .transact_to_graph(&[d.clone()], TS, None, Some("owl:materialize"), graph)
        .unwrap();
    // Historical databases may contain repeated claims by one source even
    // though new same-source assertions are idempotent. Construct those exact
    // physical rows; production retraction must preserve both boundaries.
    let middle = store
        .transact(&[], TS, None, Some("owl:materialize"))
        .unwrap();
    let late = store
        .transact(&[], TS, None, Some("owl:materialize"))
        .unwrap();
    for tx in [middle, late] {
        store
            .conn
            .execute(
                "INSERT INTO facts(e,a,v,g,tx,valid_from,op) VALUES(?1,?2,?3,?4,?5,?6,1)",
                params![d.entity, d.attribute, d.value.to_bytes(), graph, tx, TS],
            )
            .unwrap();
    }
    let foreign = store
        .transact_to_graph(&[d.clone()], TS, None, Some("reasoner:other"), graph)
        .unwrap();
    apply(
        &mut store,
        &d,
        "owl:materialize",
        graph,
        ClaimWindow {
            after: early,
            through: middle,
        },
    )
    .unwrap();
    assert_eq!(live(&store, graph, early), 1);
    assert_eq!(live(&store, graph, middle), 0);
    assert_eq!(live(&store, graph, late), 1);
    assert_eq!(live(&store, graph, foreign), 1);
    assert!(
        store
            .has_fact_claim(d.entity, d.attribute, &d.value, graph)
            .unwrap()
    );
}

#[test]
fn claim_window_never_closes_root_or_foreign_source_claims() {
    let mut store = Store::open_in_memory().unwrap();
    let d = datum(&mut store, "only");
    let graph = store.ensure_companion_inferred_graph(0, TS).unwrap();
    let root = store
        .transact(&[d.clone()], TS, None, Some("owl:materialize"))
        .unwrap();
    let selected = store
        .transact_to_graph(&[d.clone()], TS, None, Some("owl:materialize"), graph)
        .unwrap();
    let foreign = store
        .transact_to_graph(&[d.clone()], TS, None, Some("reasoner:other"), graph)
        .unwrap();
    apply(
        &mut store,
        &d,
        "owl:materialize",
        graph,
        ClaimWindow {
            after: 0,
            through: foreign,
        },
    )
    .unwrap();
    assert_eq!(live(&store, 0, root), 1);
    assert_eq!(live(&store, graph, selected), 0);
    assert_eq!(live(&store, graph, foreign), 1);
}

#[test]
fn invalid_claim_windows_leave_transaction_head_unchanged() {
    let mut store = Store::open_in_memory().unwrap();
    let d = datum(&mut store, "unchanged");
    let before = store.latest_tx_id().unwrap();
    for (after, through) in [(-1, 1), (1, 1), (2, 1)] {
        assert!(
            apply(
                &mut store,
                &d,
                "owl:materialize",
                0,
                ClaimWindow { after, through }
            )
            .is_err()
        );
        assert_eq!(store.latest_tx_id().unwrap(), before);
    }
}

#[cfg(feature = "shacl")]
#[test]
fn claim_window_runs_deletion_floor_gate_and_preserves_outside_claims() {
    let mut store = Store::open_in_memory().unwrap();
    let d = datum(&mut store, "required");
    let class = store.intern("urn:window:Class").unwrap();
    let mut kind = d.clone();
    kind.attribute = store.intern(crate::namespace::RDF_TYPE).unwrap();
    kind.value = Value::Ref(class);
    store.transact(&[kind], TS, None, Some("types")).unwrap();
    let selected = store
        .transact(&[d.clone()], TS, None, Some("owl:materialize"))
        .unwrap();
    store
        .load_shapes(
            "floor",
            r#"
        @prefix sh: <http://www.w3.org/ns/shacl#> .
        <urn:window:Shape> a sh:NodeShape; sh:targetClass <urn:window:Class>;
          sh:property [sh:path <urn:window:value>; sh:minCount 1] .
    "#,
            TS,
        )
        .unwrap();
    store.shacl_config_mut().validate_on_write = true;
    let window = ClaimWindow {
        after: 0,
        through: selected,
    };
    assert!(apply(&mut store, &d, "owl:materialize", 0, window).is_err());
    assert_eq!(live(&store, 0, selected), 1);
    assert!(
        store
            .refusals_by_gate()
            .unwrap()
            .iter()
            .any(|(gate, n)| gate == "shacl" && *n > 0),
        "deletion-floor refusal survives the write rollback"
    );
    // A historical duplicate under the SAME source, outside the upper bound,
    // satisfies the floor. The naive source-only preflight wrongly refuses it.
    let outside = store
        .transact(&[], TS, None, Some("owl:materialize"))
        .unwrap();
    store
        .conn
        .execute(
            "INSERT INTO facts(e,a,v,g,tx,valid_from,op) VALUES(?1,?2,?3,0,?4,?5,1)",
            params![d.entity, d.attribute, d.value.to_bytes(), outside, TS],
        )
        .unwrap();
    apply(&mut store, &d, "owl:materialize", 0, window).unwrap();
    assert_eq!(live(&store, 0, selected), 0);
    assert_eq!(live(&store, 0, outside), 1);
}
