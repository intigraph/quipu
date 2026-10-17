use super::*;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization, TransactionOperation};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[test]
fn concurrent_foreign_assertion_is_never_reconciled_away() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.db");
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    crate::tool_knot(
        &mut store,
        &json!({"turtle":"<urn:a> <urn:link> <urn:b> ."}),
    )
    .unwrap();
    let input = json!({"algorithm":"louvain", "persist":true});
    let first = crate::tool_project(&mut store, &input).unwrap();
    let g = store
        .registered_graph_id(first["derived_graph"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let e = store.intern("urn:foreign").unwrap();
    let a = store.intern("urn:note").unwrap();
    let fired = Arc::new(AtomicBool::new(false));
    let observed = fired.clone();
    store
        .conn
        .authorizer(Some(move |ctx: AuthContext<'_>| {
            if matches!(
                ctx.action,
                AuthAction::Savepoint {
                    operation: TransactionOperation::Begin,
                    savepoint_name: "quipu_community_run"
                }
            ) && !observed.swap(true, Ordering::SeqCst)
            {
                // SQLite's real prepare boundary: another legitimate connection
                // commits AFTER the foreign check but BEFORE this savepoint.
                let mut other = Store::open(path.to_str().unwrap()).unwrap();
                other
                    .transact_to_graph(
                        &[Datum {
                            entity: e,
                            attribute: a,
                            value: Value::Str("must retain".into()),
                            valid_from: "2026-01-03".into(),
                            valid_to: None,
                            op: Op::Assert,
                        }],
                        "2026-01-03",
                        None,
                        Some("foreign-reviewer-control"),
                        g,
                    )
                    .unwrap();
            }
            Authorization::Allow
        }))
        .unwrap();
    let result = crate::tool_project(&mut store, &input);
    assert!(
        fired.load(Ordering::SeqCst),
        "interleaving control did not fire"
    );
    let retained = store
        .entity_facts_in_graph(e, g)
        .unwrap()
        .iter()
        .any(|f| f.value == Value::Str("must retain".into()));
    assert!(
        result.is_err() || retained,
        "successful reconciliation erased concurrently committed foreign assertion"
    );
    assert!(
        result.is_err(),
        "concurrent generation must refuse persistence"
    );
    assert!(retained, "committed foreign data must survive refusal");
    assert!(store.conn.is_autocommit(), "refusal leaked a transaction");
}

#[test]
fn stale_projection_generation_refuses_without_creating_graph() {
    let mut store = Store::open_in_memory().unwrap();
    let before = store.latest_tx_id().unwrap();
    crate::tool_knot(
        &mut store,
        &json!({"turtle":"<urn:a> <urn:link> <urn:b> ."}),
    )
    .unwrap();
    let result = persist(
        &mut store,
        "urn:derived:stale",
        &[],
        &json!({}),
        0.0,
        "2026-01-03",
        before,
    );
    assert!(result.is_err());
    assert!(
        store
            .registered_graph_id("urn:derived:stale")
            .unwrap()
            .is_none()
    );
    assert!(store.conn.is_autocommit());
}

#[test]
fn foreign_commit_after_snapshot_refuses_write_upgrade_without_loss() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.db");
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    crate::tool_knot(
        &mut store,
        &json!({"turtle":"<urn:a> <urn:link> <urn:b> ."}),
    )
    .unwrap();
    let input = json!({"algorithm":"louvain", "persist":true});
    let first = crate::tool_project(&mut store, &input).unwrap();
    let g = store
        .registered_graph_id(first["derived_graph"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let e = store.intern("urn:foreign").unwrap();
    let a = store.intern("urn:note").unwrap();
    let active = Arc::new(AtomicBool::new(false));
    let armed = active.clone();
    let fired = Arc::new(AtomicBool::new(false));
    let observed = fired.clone();
    store
        .conn
        .authorizer(Some(move |ctx: AuthContext<'_>| {
            if matches!(
                ctx.action,
                AuthAction::Savepoint {
                    operation: TransactionOperation::Begin,
                    savepoint_name: "quipu_community_run"
                }
            ) {
                armed.store(true, Ordering::SeqCst);
            }
            if armed.load(Ordering::SeqCst)
                && matches!(
                    ctx.action,
                    AuthAction::Read {
                        table_name: "facts",
                        column_name: "e"
                    }
                )
                && !observed.swap(true, Ordering::SeqCst)
            {
                let mut other = Store::open(path.to_str().unwrap()).unwrap();
                other
                    .transact_to_graph(
                        &[Datum {
                            entity: e,
                            attribute: a,
                            value: Value::Str("must retain after snapshot".into()),
                            valid_from: "2026-01-03".into(),
                            valid_to: None,
                            op: Op::Assert,
                        }],
                        "2026-01-03",
                        None,
                        Some("foreign-reviewer-control"),
                        g,
                    )
                    .unwrap();
            }
            Authorization::Allow
        }))
        .unwrap();
    let result = crate::tool_project(&mut store, &input);
    assert!(
        fired.load(Ordering::SeqCst),
        "post-snapshot interleaving did not fire"
    );
    assert!(
        result.is_err(),
        "snapshot invalidation must refuse write upgrade"
    );
    assert!(store.conn.is_autocommit());
    assert!(
        store
            .entity_facts_in_graph(e, g)
            .unwrap()
            .iter()
            .any(|f| f.value == Value::Str("must retain after snapshot".into()))
    );
}
