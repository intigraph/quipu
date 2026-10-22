//! aegis-tg5 F2: a projection is maintained inside the writing transaction,
//! equals a fresh recompute after any sequence of writes, and refuses rather
//! than answer from a stale or undeclared definition.

use serde_json::json;

use super::{ProjectionAggregate, validate};
use crate::Store;
use crate::config::{ProjectionColumnConfig, ProjectionConfig};
use crate::types::Value;

const EX: &str = "http://ex.test/";

fn def() -> ProjectionConfig {
    ProjectionConfig {
        name: "items".into(),
        key: format!("{EX}status"),
        columns: vec![
            ProjectionColumnConfig {
                name: "priority".into(),
                predicate: format!("{EX}priority"),
                time: false,
            },
            ProjectionColumnConfig {
                name: "closed".into(),
                predicate: format!("{EX}endTime"),
                time: true,
            },
        ],
    }
}

fn knot(store: &mut Store, ttl: &str) -> serde_json::Value {
    crate::tool_knot(
        store,
        &json!({"turtle": format!("@prefix ex: <{EX}> . @prefix xsd: <http://www.w3.org/2001/XMLSchema#> . {ttl}"),
                "timestamp": "2026-10-22T00:00:00Z"}),
    )
    .unwrap()
}

fn seeded() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    knot(
        &mut store,
        r#"ex:a ex:status "open" ; ex:priority 1 .
                        ex:b ex:status "open" ; ex:priority 2 .
                        ex:c ex:status "closed" ; ex:priority 2 ; ex:endTime "2026-10-20T08:00:00Z"^^xsd:dateTime .
                        ex:d ex:status "closed" ; ex:endTime "2026-10-19T08:00:00Z"^^xsd:dateTime ."#,
    );
    store
}

fn count(
    store: &Store,
    key_in: &[serde_json::Value],
    group: Option<&str>,
    since: Option<(&str, &str)>,
) -> serde_json::Value {
    store
        .aggregate_projection(
            &def(),
            &ProjectionAggregate {
                graph: "",
                key_in,
                group_by: group,
                since,
                max_groups: 0,
            },
        )
        .unwrap()
}

#[test]
fn install_builds_and_counts_by_key_and_group() {
    let store = seeded();
    let status = store.install_projections(&[def()]).unwrap();
    assert_eq!(status[0].action, "built");
    assert_eq!(status[0].rows, 4);
    assert_eq!(count(&store, &[json!("open")], None, None)["total"], 2);
    let by = count(&store, &[], Some("key"), None);
    assert_eq!(by["total"], 4);
    let groups = by["groups"].as_array().unwrap();
    assert!(groups.contains(&json!({"value": "open", "n": 2})));
    assert!(groups.contains(&json!({"value": "closed", "n": 2})));
    let p = count(&store, &[json!("open")], Some("priority"), None);
    assert!(
        p["groups"]
            .as_array()
            .unwrap()
            .contains(&json!({"value": 1, "n": 1}))
    );
}

#[test]
fn a_committed_write_updates_its_row_in_the_same_transaction() {
    let mut store = seeded();
    store.install_projections(&[def()]).unwrap();
    knot(&mut store, r#"ex:e ex:status "open" ; ex:priority 3 ."#);
    assert_eq!(count(&store, &[json!("open")], None, None)["total"], 3);
    assert_eq!(store.check_projection(&def()).unwrap(), 0);
}

#[test]
fn a_status_change_through_retraction_moves_the_counts() {
    let mut store = seeded();
    store.install_projections(&[def()]).unwrap();
    let a = store.lookup(&format!("{EX}a")).unwrap().unwrap();
    let st = store.lookup(&format!("{EX}status")).unwrap().unwrap();
    store
        .retract_triples(
            a,
            Some(st),
            Some(&Value::Str("open".into())),
            "2026-10-22T01:00:00Z",
            None,
            false,
            None,
        )
        .unwrap();
    knot(
        &mut store,
        r#"ex:a ex:status "closed" ; ex:endTime "2026-10-21T09:30:00+02:00"^^xsd:dateTime ."#,
    );
    assert_eq!(count(&store, &[json!("open")], None, None)["total"], 1);
    assert_eq!(count(&store, &[json!("closed")], None, None)["total"], 3);
    assert_eq!(store.check_projection(&def()).unwrap(), 0);
}

#[test]
fn since_is_offset_safe_and_uses_the_time_column() {
    let mut store = seeded();
    store.install_projections(&[def()]).unwrap();
    // 09:30+02:00 is 07:30Z: inside a window that starts at 07:00Z.
    knot(
        &mut store,
        r#"ex:f ex:status "closed" ; ex:endTime "2026-10-21T09:30:00+02:00"^^xsd:dateTime ."#,
    );
    let n = |at| count(&store, &[json!("closed")], None, Some(("closed", at)))["total"].clone();
    assert_eq!(n("2026-10-21T07:00:00Z"), 1);
    assert_eq!(n("2026-10-21T07:31:00Z"), 0);
    assert_eq!(n("2026-10-20T00:00:00Z"), 2);
    let err = store
        .aggregate_projection(
            &def(),
            &ProjectionAggregate {
                since: Some(("priority", "2026-10-21T00:00:00Z")),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("not a time column"), "{err}");
}

#[test]
fn check_detects_drift_and_rebuild_repairs_it() {
    let store = seeded();
    store.install_projections(&[def()]).unwrap();
    store
        .conn
        .execute(
            "DELETE FROM qp_proj_items WHERE rowid IN (SELECT rowid FROM qp_proj_items LIMIT 1)",
            [],
        )
        .ok();
    store.conn.execute("UPDATE qp_proj_items SET key = X'01626F677573' WHERE e = (SELECT min(e) FROM qp_proj_items)", []).unwrap();
    assert!(
        store.check_projection(&def()).unwrap() > 0,
        "a corrupted row must be visible"
    );
    assert_eq!(store.rebuild_projection(&def()).unwrap(), 4);
    assert_eq!(store.check_projection(&def()).unwrap(), 0);
}

#[test]
fn a_changed_declaration_rebuilds_and_an_undeclared_one_is_dropped() {
    let store = seeded();
    store.install_projections(&[def()]).unwrap();
    assert_eq!(
        store.install_projections(&[def()]).unwrap()[0].action,
        "unchanged"
    );
    let mut wider = def();
    wider.columns.pop();
    assert_eq!(
        store.install_projections(&[wider]).unwrap()[0].action,
        "built"
    );
    let out = store.install_projections(&[]).unwrap();
    assert_eq!(out[0].action, "dropped");
    let left: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE 'qp_proj_items%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(left, 0, "table, indexes and triggers all gone");
}

#[test]
fn an_uninstalled_or_stale_definition_refuses_to_answer() {
    let store = seeded();
    let err = store
        .aggregate_projection(&def(), &ProjectionAggregate::default())
        .unwrap_err();
    assert!(matches!(err, crate::Error::CannotVerify(_)), "{err}");
    store.install_projections(&[def()]).unwrap();
    let mut other = def();
    other.columns.clear();
    let err = store
        .aggregate_projection(&other, &ProjectionAggregate::default())
        .unwrap_err();
    assert!(matches!(err, crate::Error::CannotVerify(_)), "{err}");
}

#[test]
fn a_multi_valued_key_is_flagged_not_silently_resolved() {
    let mut store = seeded();
    store.install_projections(&[def()]).unwrap();
    knot(&mut store, r#"ex:b ex:status "blocked" ."#);
    let all = count(&store, &[], None, None);
    assert_eq!(all["multi_valued_rows"], 1);
}

#[test]
fn names_that_reach_sql_are_validated() {
    for bad in ["Items", "x; DROP TABLE facts", "", "9lives", "a-b"] {
        let mut d = def();
        d.name = bad.into();
        assert!(validate(&d).is_err(), "{bad:?} accepted");
    }
    let mut d = def();
    d.columns[0].name = "key".into();
    assert!(validate(&d).is_err());
    let mut d = def();
    d.columns[1].name = "priority".into();
    assert!(validate(&d).is_err(), "duplicate column accepted");
}

#[test]
fn a_refused_write_leaves_the_projection_untouched() {
    let mut store = seeded();
    store.install_projections(&[def()]).unwrap();
    let shapes = format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix ex: <{EX}> .
         ex:S a sh:NodeShape ; sh:targetSubjectsOf ex:status ;
             sh:property [ sh:path ex:priority ; sh:maxCount 1 ] ."
    );
    let refused = crate::tool_knot(
        &mut store,
        &json!({"turtle": format!("@prefix ex: <{EX}> . ex:g ex:status \"open\" ; ex:priority 1, 2 ."),
                "shapes": shapes, "timestamp": "2026-10-22T00:00:00Z"}),
    );
    let refused = match refused {
        Err(_) => true,
        Ok(r) => r["conforms"] == json!(false),
    };
    assert!(refused, "the control write must be refused");
    assert_eq!(
        count(&store, &[json!("open")], None, None)["total"],
        2,
        "no row from a refused write"
    );
    assert_eq!(store.check_projection(&def()).unwrap(), 0);
}
