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
                graphs: &[],
                key_in,
                filters: &[],
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
    assert!(groups.contains(&json!({"value": "open", "n": 2, "plain": true})));
    assert!(groups.contains(&json!({"value": "closed", "n": 2, "plain": true})));
    let p = count(&store, &[json!("open")], Some("priority"), None);
    assert!(
        p["groups"]
            .as_array()
            .unwrap()
            .contains(&json!({"value": 1, "n": 1, "plain": false}))
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

#[test]
fn an_aggregate_reads_one_snapshot_while_writes_land() {
    // Review finding on #734: total, groups and tx came from separate
    // statements, so a write landing between them returned a total from one
    // state with groups from another. Land a write through a second
    // connection exactly between the total and the groups: the aggregate must
    // still be internally consistent (total = sum of groups) and must not see
    // that write at all.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("q.db");
    let path = path.to_str().unwrap().to_string();
    let mut reader = Store::open(&path).unwrap();
    knot(&mut reader, r#"ex:a ex:status "open" ; ex:priority 1 ."#);
    reader.install_projections(&[def()]).unwrap();
    let mut writer = Store::open(&path).unwrap();
    super::set_after_total(Some(Box::new(move || {
        knot(&mut writer, r#"ex:late ex:status "open" ; ex:priority 1 ."#);
    })));
    let q = ProjectionAggregate {
        graph: "",
        graphs: &[],
        key_in: &[json!("open")],
        filters: &[],
        group_by: Some("priority"),
        since: None,
        max_groups: 0,
    };
    let a = reader.aggregate_projection(&def(), &q);
    super::set_after_total(None);
    let a = a.unwrap();
    let sum: i64 = a["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["n"].as_i64().unwrap())
        .sum();
    assert_eq!(
        (a["total"].as_i64().unwrap(), sum),
        (1, 1),
        "one snapshot: {a}"
    );
    // The write did land; a fresh aggregate sees it.
    let after = reader.aggregate_projection(&def(), &q).unwrap();
    assert_eq!(after["total"], 2, "{after}");
}

fn filtered(
    store: &Store,
    filters: &[(&str, &[serde_json::Value])],
    group: Option<&str>,
) -> serde_json::Value {
    store
        .aggregate_projection(
            &def(),
            &ProjectionAggregate {
                filters,
                group_by: group,
                ..Default::default()
            },
        )
        .unwrap()
}

fn groups(a: &serde_json::Value) -> Vec<(serde_json::Value, i64)> {
    let mut g: Vec<_> = a["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| (g["value"].clone(), g["n"].as_i64().unwrap()))
        .collect();
    g.sort_by_key(|(v, _)| v.to_string());
    g
}

#[test]
fn column_filters_and_across_columns_or_within_one() {
    // aegis-tg5 7mm shape: two column conditions combined, grouped by a third.
    let mut store = seeded();
    store.install_projections(&[def()]).unwrap();
    let a = filtered(
        &store,
        &[("key", &[json!("open")]), ("priority", &[json!(2)])],
        None,
    );
    assert_eq!(a["total"], 1, "{a}");
    let a = filtered(&store, &[("priority", &[json!(2)])], Some("key"));
    assert_eq!(a["total"], 2);
    assert_eq!(groups(&a), vec![(json!("closed"), 1), (json!("open"), 1)]);
    let a = filtered(&store, &[("priority", &[json!(1), json!(2)])], Some("key"));
    assert_eq!(groups(&a), vec![(json!("closed"), 1), (json!("open"), 2)]);
    // null = no value for the column: ex:d has no priority.
    let a = filtered(
        &store,
        &[("priority", &[serde_json::Value::Null])],
        Some("key"),
    );
    assert_eq!(groups(&a), vec![(json!("closed"), 1)]);
    // An empty alternative list matches nothing; an unknown IRI cannot match.
    assert_eq!(filtered(&store, &[("priority", &[])], None)["total"], 0);
    let unknown = [json!({"iri": "http://ex.test/never"})];
    assert_eq!(
        filtered(&store, &[("priority", &unknown)], None)["total"],
        0
    );
    // The projection stays maintained: a new write is counted by the filter.
    knot(&mut store, r#"ex:e ex:status "open" ; ex:priority 2 ."#);
    let a = filtered(
        &store,
        &[("key", &[json!("open")]), ("priority", &[json!(2)])],
        None,
    );
    assert_eq!(a["total"], 2, "{a}");
}

#[test]
fn column_filters_refuse_undeclared_columns_and_bad_values() {
    let store = seeded();
    store.install_projections(&[def()]).unwrap();
    for (filters, needle) in [
        (vec![("assignee", vec![json!("wu")])], "has no column"),
        (
            vec![("priority; DROP TABLE facts", vec![json!(1)])],
            "has no column",
        ),
        (vec![("priority", vec![json!(true)])], "filter values"),
        (
            vec![("priority", vec![json!({"not": "iri"})])],
            "needs \"iri\"",
        ),
    ] {
        let filters: Vec<(&str, &[serde_json::Value])> =
            filters.iter().map(|(c, v)| (*c, v.as_slice())).collect();
        let err = store
            .aggregate_projection(
                &def(),
                &ProjectionAggregate {
                    filters: &filters,
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(err.to_string().contains(needle), "{err}");
    }
}

#[test]
fn an_input_that_matches_nothing_does_not_hide_a_later_invalid_one() {
    // Review finding on the filter change: an empty filter answered a
    // successful zero before a later undeclared column was checked.
    let store = seeded();
    store.install_projections(&[def()]).unwrap();
    let unknown = [json!({"iri": "http://ex.test/never"})];
    let empty: [serde_json::Value; 0] = [];
    let one = [json!(1)];
    let bad = [json!(true)];
    let empty_then_undeclared = [("priority", &empty[..]), ("assignee", &one[..])];
    let unknown_then_bad = [("priority", &unknown[..]), ("closed", &bad[..])];
    let undeclared = [("assignee", &one[..])];
    let only_empty = [("priority", &empty[..])];
    let cases: Vec<(ProjectionAggregate<'_>, &str)> = vec![
        (
            ProjectionAggregate {
                filters: &empty_then_undeclared,
                ..Default::default()
            },
            "has no column",
        ),
        (
            ProjectionAggregate {
                filters: &unknown_then_bad,
                ..Default::default()
            },
            "filter values",
        ),
        (
            ProjectionAggregate {
                key_in: &unknown,
                group_by: Some("team"),
                ..Default::default()
            },
            "has no column",
        ),
        (
            ProjectionAggregate {
                graph: "http://ex.test/no-such-graph",
                since: Some(("priority", "2026-10-21T00:00:00Z")),
                ..Default::default()
            },
            "not a time column",
        ),
        (
            ProjectionAggregate {
                graph: "http://ex.test/no-such-graph",
                filters: &undeclared,
                ..Default::default()
            },
            "has no column",
        ),
    ];
    for (q, needle) in cases {
        let err = store.aggregate_projection(&def(), &q).unwrap_err();
        assert!(err.to_string().contains(needle), "{needle}: {err}");
    }
    // The valid nothing-matching forms still answer a plain zero.
    for q in [
        ProjectionAggregate {
            filters: &only_empty,
            group_by: Some("key"),
            ..Default::default()
        },
        ProjectionAggregate {
            key_in: &unknown,
            ..Default::default()
        },
        ProjectionAggregate {
            graph: "http://ex.test/no-such-graph",
            group_by: Some("priority"),
            ..Default::default()
        },
    ] {
        let a = store.aggregate_projection(&def(), &q).unwrap();
        assert_eq!(a["total"], 0, "{a}");
        assert_eq!(a["groups"], json!([]));
    }
}
