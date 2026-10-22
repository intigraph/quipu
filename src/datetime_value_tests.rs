use std::cmp::Ordering;

use super::{both_date_times, compare};
use crate::{Value, namespace};

fn dt(s: &str) -> Value {
    Value::Typed {
        lexical: s.into(),
        datatype: namespace::XSD_DATE_TIME.into(),
    }
}

fn cmp(a: &str, b: &str) -> Option<Ordering> {
    compare(&dt(a), &dt(b))
}

#[test]
fn the_same_instant_with_different_offsets_is_equal() {
    assert_eq!(
        cmp("2026-10-22T01:00:00+02:00", "2026-10-21T23:00:00Z"),
        Some(Ordering::Equal)
    );
    assert_eq!(
        cmp("2026-10-21T18:00:00-05:00", "2026-10-21T23:00:00Z"),
        Some(Ordering::Equal)
    );
}

#[test]
fn ordering_follows_the_timeline_not_the_lexeme() {
    // Lexically "2026-10-22..." > "2026-10-21T23:30Z"; on the timeline it is 23:00Z.
    assert_eq!(
        cmp("2026-10-22T01:00:00+02:00", "2026-10-21T23:30:00Z"),
        Some(Ordering::Less)
    );
    assert_eq!(
        cmp("2026-10-21T23:30:00-01:00", "2026-10-22T00:15:00Z"),
        Some(Ordering::Greater)
    );
}

#[test]
fn fractional_seconds_order_and_trailing_zeros_do_not_matter() {
    assert_eq!(
        cmp("2026-10-21T10:00:00.5Z", "2026-10-21T10:00:00.25Z"),
        Some(Ordering::Greater)
    );
    assert_eq!(
        cmp("2026-10-21T10:00:00.50Z", "2026-10-21T10:00:00.5Z"),
        Some(Ordering::Equal)
    );
    assert_eq!(
        cmp("2026-10-21T10:00:00Z", "2026-10-21T10:00:00.000Z"),
        Some(Ordering::Equal)
    );
}

#[test]
fn twenty_four_hundred_is_the_next_midnight() {
    assert_eq!(
        cmp("2026-10-21T24:00:00Z", "2026-10-22T00:00:00Z"),
        Some(Ordering::Equal)
    );
    assert!(!both_date_times(
        &dt("2026-10-21T24:00:01Z"),
        &dt("2026-10-22T00:00:00Z")
    ));
}

#[test]
fn untimezoned_values_compare_with_each_other_directly() {
    assert_eq!(
        cmp("2026-10-21T10:00:00", "2026-10-21T09:00:00"),
        Some(Ordering::Greater)
    );
}

#[test]
fn an_untimezoned_operand_takes_the_implicit_timezone_utc() {
    // XPath op:dateTime-* give a missing timezone the implicit one (UTC here),
    // so the order is total: 12:00 (implicitly Z) is after 10:00Z.
    assert_eq!(
        cmp("2026-10-21T10:00:00Z", "2026-10-21T12:00:00"),
        Some(Ordering::Less)
    );
    assert_eq!(
        cmp("2026-10-21T12:00:00", "2026-10-21T12:00:00Z"),
        Some(Ordering::Equal)
    );
    assert_eq!(
        cmp("2026-10-21T12:00:00", "2026-10-21T13:00:00+01:00"),
        Some(Ordering::Equal)
    );
}

#[test]
fn fractions_compare_exactly_at_any_precision() {
    // Review finding: 9-digit truncation made these equal.
    assert_eq!(
        cmp(
            "2026-10-21T10:00:00.0000000001Z",
            "2026-10-21T10:00:00.0000000002Z"
        ),
        Some(Ordering::Less)
    );
    assert_eq!(
        cmp(
            "2026-10-21T10:00:00.12345678901Z",
            "2026-10-21T10:00:00.123456789010Z"
        ),
        Some(Ordering::Equal)
    );
}

#[test]
fn twenty_four_hundred_with_any_nonzero_fraction_is_invalid() {
    assert!(!both_date_times(
        &dt("2026-10-21T24:00:00.0000000001Z"),
        &dt("2026-10-22T00:00:00Z")
    ));
    assert_eq!(
        cmp("2026-10-21T24:00:00.000Z", "2026-10-22T00:00:00Z"),
        Some(Ordering::Equal)
    );
}

#[test]
fn only_parseable_date_times_take_this_path() {
    let plain = Value::Str("2026-10-21T10:00:00Z".into());
    assert!(!both_date_times(&plain, &dt("2026-10-21T10:00:00Z")));
    assert!(!both_date_times(
        &dt("not a date"),
        &dt("2026-10-21T10:00:00Z")
    ));
    assert!(!both_date_times(
        &dt("2026-02-30T10:00:00Z"),
        &dt("2026-10-21T10:00:00Z")
    ));
    assert!(!both_date_times(
        &dt("2026-10-21T10:00:00+15:00"),
        &dt("2026-10-21T10:00:00Z")
    ));
    assert!(both_date_times(
        &dt("2026-10-21T10:00:00+14:00"),
        &dt("2026-10-21T10:00:00")
    ));
}

fn count(store: &crate::Store, q: &str) -> Option<Value> {
    let r = crate::sparql::query(
        store,
        &format!(
            "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> PREFIX ex: <http://ex.test/> {q}"
        ),
    )
    .unwrap();
    r.rows()[0].get("n").cloned()
}

fn n(v: i64) -> Option<Value> {
    Some(Value::Int(v))
}

#[test]
fn a_filter_window_counts_instants_across_offsets() {
    // The aegis-dio probe: "01:00+02:00" is 23:00Z, before the 23:15Z bound.
    let store = crate::Store::open_in_memory().unwrap();
    let q = r#"SELECT (COUNT(*) AS ?n) WHERE {
        VALUES ?t { "2026-10-22T01:00:00+02:00"^^xsd:dateTime "2026-10-21T23:30:00Z"^^xsd:dateTime }
        FILTER(?t >= "2026-10-21T23:15:00Z"^^xsd:dateTime) }"#;
    assert_eq!(count(&store, q), n(1));
}

#[test]
fn equality_and_in_hold_across_offsets() {
    let store = crate::Store::open_in_memory().unwrap();
    let eq = r#"SELECT (COUNT(*) AS ?n) WHERE {
        FILTER("2026-10-22T01:00:00+02:00"^^xsd:dateTime = "2026-10-21T23:00:00Z"^^xsd:dateTime) }"#;
    assert_eq!(count(&store, eq), n(1));
    let within = r#"SELECT (COUNT(*) AS ?n) WHERE {
        FILTER("2026-10-21T18:00:00-05:00"^^xsd:dateTime IN ("2026-10-21T23:00:00Z"^^xsd:dateTime)) }"#;
    assert_eq!(count(&store, within), n(1));
}

#[test]
fn stored_facts_are_windowed_by_instant() {
    // The DORA / burn-down shape: a time window over stored endTime facts.
    let mut store = crate::Store::open_in_memory().unwrap();
    crate::tool_knot(
        &mut store,
        &serde_json::json!({"turtle": r#"@prefix ex: <http://ex.test/> . @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
            ex:a ex:endTime "2026-10-22T01:00:00+02:00"^^xsd:dateTime .
            ex:b ex:endTime "2026-10-21T23:30:00Z"^^xsd:dateTime .
            ex:c ex:endTime "2026-10-21T19:00:00-05:00"^^xsd:dateTime ."#,
            "timestamp": "2026-10-22T00:00:00Z"}),
    )
    .unwrap();
    // a = 23:00Z (out), b = 23:30Z (in), c = 00:00Z next day (in).
    let q = r#"SELECT (COUNT(*) AS ?n) WHERE {
        ?i ex:endTime ?t FILTER(?t >= "2026-10-21T23:15:00Z"^^xsd:dateTime) }"#;
    assert_eq!(count(&store, q), n(2));
}

#[test]
fn negations_follow_the_same_determinate_order() {
    // Review finding: an indeterminate pair used to be false for `=` and so TRUE
    // for `!=`, `NOT IN` and `!(<)`. With the implicit timezone there is no
    // indeterminate pair; each negation is the exact complement.
    let store = crate::Store::open_in_memory().unwrap();
    let a = r#""2026-10-21T10:00:00Z"^^xsd:dateTime"#;
    let b = r#""2026-10-21T12:00:00"^^xsd:dateTime"#;
    let same = r#""2026-10-21T13:00:00+01:00"^^xsd:dateTime"#;
    for (expr, want) in [
        (format!("{a} != {b}"), 1),
        (format!("{b} != {same}"), 0),
        (format!("!({a} < {b})"), 0),
        (format!("{b} NOT IN ({same})"), 0),
        (format!("{a} NOT IN ({b})"), 1),
    ] {
        let q = format!("SELECT (COUNT(*) AS ?n) WHERE {{ FILTER({expr}) }}");
        assert_eq!(count(&store, &q), n(want), "{expr}");
    }
}
