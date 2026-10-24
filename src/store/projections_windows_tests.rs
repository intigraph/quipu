//! aegis-2fa: `[since, until)` windows, UTC day buckets with a second
//! dimension, a row cursor, and presence columns
//! (docs/design/projection-aggregate-windows.md).

use serde_json::json;

use super::{GroupBy, ProjectionAggregate, ProjectionRows, RowsAfter};
use crate::Store;
use crate::config::{ProjectionColumnConfig, ProjectionConfig};
use crate::types::Value;

const EX: &str = "http://ex.test/";

fn col(name: &str, predicate: &str, time: bool, equals: Option<&str>) -> ProjectionColumnConfig {
    ProjectionColumnConfig {
        name: name.into(),
        predicate: format!("{EX}{predicate}"),
        time,
        equals: equals.map(Into::into),
    }
}

fn def() -> ProjectionConfig {
    ProjectionConfig {
        name: "items".into(),
        key: format!("{EX}status"),
        columns: vec![
            col("priority", "priority", false, None),
            col("closed", "endTime", true, None),
            col("inbox", "keywords", false, Some("inbox")),
        ],
    }
}

fn knot(store: &mut Store, ttl: &str) {
    crate::tool_knot(
        store,
        &json!({"turtle": format!("@prefix ex: <{EX}> . @prefix xsd: <http://www.w3.org/2001/XMLSchema#> . {ttl}"),
                "timestamp": "2026-10-22T00:00:00Z"}),
    )
    .unwrap();
}

/// Closures around a UTC midnight, with offsets and fractional seconds.
fn seeded() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    knot(
        &mut store,
        r#"ex:a ex:status "closed" ; ex:priority 1 ; ex:endTime "2026-10-20T23:59:59.900Z"^^xsd:dateTime .
           ex:b ex:status "closed" ; ex:priority 2 ; ex:endTime "2026-10-21T01:30:00+02:00"^^xsd:dateTime .
           ex:c ex:status "closed" ; ex:priority 1 ; ex:endTime "2026-10-21T00:00:00Z"^^xsd:dateTime .
           ex:d ex:status "closed" ; ex:priority 2 ; ex:endTime "2026-10-21T22:30:00-02:00"^^xsd:dateTime .
           ex:e ex:status "open" ; ex:priority 1 ; ex:keywords "triage", "inbox" .
           ex:f ex:status "open" ; ex:priority 1 ; ex:keywords "triage" ."#,
    );
    store.install_projections(&[def()]).unwrap();
    store
}

fn agg(store: &Store, q: &ProjectionAggregate<'_>) -> serde_json::Value {
    store.aggregate_projection(&def(), q).unwrap()
}

#[test]
fn since_is_inclusive_and_until_exclusive_on_offset_and_fractional_instants() {
    let store = seeded();
    let n = |since: &str, until: &str| {
        agg(
            &store,
            &ProjectionAggregate {
                since: Some(("closed", since)),
                until: Some(("closed", until)),
                ..Default::default()
            },
        )["total"]
            .clone()
    };
    // b is 2026-10-20T23:30Z and c is exactly midnight; d is 10-22T00:30Z.
    assert_eq!(
        n("2026-10-21T00:00:00Z", "2026-10-22T00:00:00Z"),
        1,
        "c only"
    );
    assert_eq!(
        n("2026-10-20T00:00:00Z", "2026-10-21T00:00:00Z"),
        2,
        "a and b, not c"
    );
    assert_eq!(
        n("2026-10-22T00:30:00Z", "2026-10-22T00:30:01Z"),
        1,
        "d at its start"
    );
    assert_eq!(
        n("2026-10-22T00:00:00Z", "2026-10-22T00:30:00Z"),
        0,
        "d excluded at its end"
    );
    let err = store
        .aggregate_projection(
            &def(),
            &ProjectionAggregate {
                until: Some(("priority", "2026-10-21T00:00:00Z")),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("until: column \"priority\" is not a time column"),
        "{err}"
    );
    let err = store
        .aggregate_projection(
            &def(),
            &ProjectionAggregate {
                until: Some(("closed", "yesterday")),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("not an RFC 3339 instant"), "{err}");
    // Bounds are whole seconds: zeros after the point are fine, a real
    // fraction is refused, on either bound.
    assert_eq!(n("2026-10-21T00:00:00.000Z", "2026-10-22T00:00:00Z"), 1);
    for (since, until) in [
        ("2026-10-21T00:00:00.5Z", "2026-10-22T00:00:00Z"),
        ("2026-10-21T00:00:00Z", "2026-10-22T00:00:00.001+02:00"),
    ] {
        let err = store
            .aggregate_projection(
                &def(),
                &ProjectionAggregate {
                    since: Some(("closed", since)),
                    until: Some(("closed", until)),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("window bounds are whole seconds"),
            "{err}"
        );
    }
}

/// The UTC day of each closure, computed without SQLite (Hinnant's
/// civil-from-days over floored epoch days).
fn utc_day(at: &str) -> String {
    let z = crate::time::epoch_of_rfc3339(at)
        .unwrap()
        .div_euclid(86_400)
        + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[test]
fn day_buckets_are_the_utc_calendar_day_computed_independently() {
    let store = seeded();
    let a = agg(
        &store,
        &ProjectionAggregate {
            key_in: &[json!("closed")],
            group_by: &[GroupBy::Day("closed")],
            ..Default::default()
        },
    );
    let mut got: Vec<(String, i64)> = a["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["value"].as_str().unwrap().to_string(),
                g["n"].as_i64().unwrap(),
            )
        })
        .collect();
    got.sort();
    let mut want = std::collections::BTreeMap::new();
    for at in [
        "2026-10-20T23:59:59.900Z",
        "2026-10-21T01:30:00+02:00",
        "2026-10-21T00:00:00Z",
        "2026-10-21T22:30:00-02:00",
    ] {
        *want.entry(utc_day(at)).or_insert(0i64) += 1;
    }
    assert_eq!(got, want.into_iter().collect::<Vec<_>>(), "{a}");
    assert_eq!(
        got,
        vec![
            ("2026-10-20".into(), 2),
            ("2026-10-21".into(), 1),
            ("2026-10-22".into(), 1)
        ]
    );
}

#[test]
fn a_two_dimension_aggregate_equals_one_dimension_counts_per_value() {
    let store = seeded();
    let two = agg(
        &store,
        &ProjectionAggregate {
            key_in: &[json!("closed")],
            group_by: &[GroupBy::Day("closed"), GroupBy::Column("priority")],
            ..Default::default()
        },
    );
    let mut cells: Vec<(String, i64, i64)> = two["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            assert!(
                g.get("value").is_none(),
                "two dimensions answer values: {g}"
            );
            (
                g["values"][0].as_str().unwrap().to_string(),
                g["values"][1].as_i64().unwrap(),
                g["n"].as_i64().unwrap(),
            )
        })
        .collect();
    cells.sort();
    let mut by_filter = Vec::new();
    for p in [1, 2] {
        let one = agg(
            &store,
            &ProjectionAggregate {
                key_in: &[json!("closed")],
                filters: &[("priority", &[json!(p)])],
                group_by: &[GroupBy::Day("closed")],
                ..Default::default()
            },
        );
        for g in one["groups"].as_array().unwrap() {
            by_filter.push((
                g["value"].as_str().unwrap().to_string(),
                p,
                g["n"].as_i64().unwrap(),
            ));
        }
    }
    by_filter.sort();
    assert_eq!(cells, by_filter, "{two}");
    for (group_by, needle) in [
        (
            vec![
                GroupBy::Column("key"),
                GroupBy::Column("priority"),
                GroupBy::Day("closed"),
            ],
            "at most 2 dimensions",
        ),
        (
            vec![GroupBy::Day("priority")],
            "group_by: column \"priority\" is not a time column",
        ),
        (vec![GroupBy::Column("team")], "has no column"),
    ] {
        let err = store
            .aggregate_projection(
                &def(),
                &ProjectionAggregate {
                    group_by: &group_by,
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(err.to_string().contains(needle), "{needle}: {err}");
    }
}

fn page(store: &Store, after: Option<RowsAfter<'_>>) -> serde_json::Value {
    store
        .projection_rows(
            &def(),
            &ProjectionRows {
                columns: Some(&["priority"]),
                after,
                limit: 2,
                ..Default::default()
            },
        )
        .unwrap()
}

#[test]
fn paging_with_after_returns_every_row_once_at_one_tx() {
    let mut store = seeded();
    let mut seen = Vec::new();
    let mut cursor: Option<serde_json::Value> = None;
    let mut txs = std::collections::BTreeSet::new();
    loop {
        let p = match &cursor {
            None => page(&store, None),
            Some(c) => page(
                &store,
                Some(RowsAfter {
                    entity: c["entity"].as_str().unwrap(),
                    graph: c["graph"].as_str().unwrap(),
                }),
            ),
        };
        txs.insert(p["tx"].to_string());
        assert_eq!(
            p["multi_valued_rows"], 0,
            "presence never marks a row multi: {p}"
        );
        seen.extend(
            p["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["entity"].as_str().unwrap().to_string()),
        );
        if p["truncated"] == false {
            assert!(p.get("next_after").is_none(), "{p}");
            break;
        }
        cursor = Some(p["next_after"].clone());
    }
    let mut want: Vec<String> = "abcdef".chars().map(|c| format!("{EX}{c}")).collect();
    let mut sorted = seen.clone();
    sorted.sort();
    want.sort();
    assert_eq!(sorted, want, "every row exactly once");
    assert_eq!(seen.len(), 6);
    assert_eq!(txs.len(), 1, "unchanged store, one tx");
    // An interleaved write moves tx: the client contract is restart.
    let first = page(&store, None);
    knot(&mut store, r#"ex:g ex:status "open" ."#);
    let next = page(
        &store,
        Some(RowsAfter {
            entity: first["next_after"]["entity"].as_str().unwrap(),
            graph: first["next_after"]["graph"].as_str().unwrap(),
        }),
    );
    assert_ne!(first["tx"], next["tx"]);
    for (after, needle) in [
        (
            RowsAfter {
                entity: "http://ex.test/nobody",
                graph: "",
            },
            "unknown entity",
        ),
        (
            RowsAfter {
                entity: "http://ex.test/a",
                graph: "http://ex.test/other-graph",
            },
            "not in this listing's scope",
        ),
    ] {
        let err = store
            .projection_rows(
                &def(),
                &ProjectionRows {
                    after: Some(after),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(err.to_string().contains(needle), "{needle}: {err}");
    }
}

#[test]
fn a_presence_column_sees_one_value_among_several_and_follows_retraction() {
    let mut store = seeded();
    let inbox = |store: &Store, v: serde_json::Value| {
        agg(
            store,
            &ProjectionAggregate {
                key_in: &[json!("open")],
                filters: &[("inbox", &[v])],
                ..Default::default()
            },
        )["total"]
            .clone()
    };
    // e has "triage" and "inbox"; f has only "triage".
    assert_eq!(inbox(&store, json!(1)), 1);
    assert_eq!(inbox(&store, json!(null)), 1);
    let e = store.lookup(&format!("{EX}e")).unwrap().unwrap();
    let kw = store.lookup(&format!("{EX}keywords")).unwrap().unwrap();
    store
        .retract_triples(
            e,
            Some(kw),
            Some(&Value::Str("inbox".into())),
            "2026-10-22T01:00:00Z",
            None,
            false,
            None,
        )
        .unwrap();
    assert_eq!(inbox(&store, json!(1)), 0);
    assert_eq!(inbox(&store, json!(null)), 2);
    // Exact equality: another case is another value.
    knot(&mut store, r#"ex:f ex:keywords "Inbox" ."#);
    assert_eq!(inbox(&store, json!(1)), 0);
    knot(&mut store, r#"ex:f ex:keywords "inbox" ."#);
    assert_eq!(inbox(&store, json!(1)), 1);
    assert_eq!(store.check_projection(&def()).unwrap(), 0);
}

#[test]
fn a_presence_column_cannot_be_a_time_column_and_does_not_rehash_others() {
    let mut bad = def();
    bad.columns[2].time = true;
    let err = super::validate(&bad).unwrap_err();
    assert!(err.to_string().contains("presence column"), "{err}");
    // A declaration without presence columns hashes as it did before.
    let mut plain = def();
    plain.columns.truncate(2);
    let mut with = plain.clone();
    with.columns
        .push(col("inbox", "keywords", false, Some("inbox")));
    assert_ne!(super::def_hash(&plain), super::def_hash(&with));
    assert_eq!(super::def_hash(&plain), super::def_hash(&plain.clone()));
}
