//! FILTER comparisons on temporal literals compare by value (aegis-jd2t).
use super::query;
use crate::{
    Store,
    store::Datum,
    types::{Op, Value},
};

const TS: &str = "2026-01-01T00:00:00Z";
const XSD: &str = "http://www.w3.org/2001/XMLSchema#";

fn store_with(values: &[(&str, &str)]) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let a = store.intern("urn:t").unwrap();
    let datums: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(i, (lexical, datatype))| Datum {
            entity: store.intern(&format!("urn:e:{i}")).unwrap(),
            attribute: a,
            value: if datatype.is_empty() {
                Value::Str((*lexical).into())
            } else {
                Value::Typed {
                    lexical: (*lexical).into(),
                    datatype: format!("{XSD}{datatype}"),
                }
            },
            valid_from: TS.into(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    store.transact(&datums, TS, None, None).unwrap();
    store
}

/// Lexical forms of the values that pass `?t <op> <bound>`, sorted.
fn passing(store: &Store, op: &str, bound: &str) -> Vec<String> {
    let q = format!("SELECT ?t WHERE {{ ?e <urn:t> ?t FILTER(?t {op} {bound}) }}");
    let mut out: Vec<String> = query(store, &q)
        .unwrap()
        .rows()
        .iter()
        .map(|r| match &r["t"] {
            Value::Typed { lexical, .. } | Value::Str(lexical) => lexical.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    out.sort();
    out
}

const T: &str = "\"2026-10-20T10:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime>";

#[test]
fn date_times_compare_by_instant_and_ill_typed_values_never_pass() {
    let store = store_with(&[
        ("2026-10-20T06:00:00-05:00", "dateTime"), // 11:00Z: after, lexically before
        ("2026-10-21T00:30:00.250+02:00", "dateTime"), // 22:30:00.25Z: after
        ("2026-10-20T10:00:00Z", "dateTime"),      // equal
        ("2026-10-19T23:59:59Z", "dateTime"),      // before
        ("not-a-time", "dateTime"),                // ill-typed: a type error
    ]);
    assert_eq!(
        passing(&store, ">=", T),
        vec![
            "2026-10-20T06:00:00-05:00",
            "2026-10-20T10:00:00Z",
            "2026-10-21T00:30:00.250+02:00"
        ]
    );
    // The ill-typed value fails the inverse too: it is not "before" either.
    assert_eq!(passing(&store, "<", T), vec!["2026-10-19T23:59:59Z"]);
}

#[test]
fn untimezoned_against_timezoned_is_indeterminate_near_the_boundary_only() {
    let store = store_with(&[
        ("2026-10-20T10:00:00", "dateTime"), // within +/-14h of the bound: indeterminate
        ("2026-11-01T00:00:00", "dateTime"), // far after: determinate
    ]);
    assert_eq!(passing(&store, ">=", T), vec!["2026-11-01T00:00:00"]);
    assert_eq!(passing(&store, "<", T), Vec::<String>::new());
}

#[test]
fn dates_compare_by_value_and_mixed_types_are_a_type_error() {
    let store = store_with(&[
        ("2026-10-19", "date"),
        ("2026-10-21", "date"),
        ("2026-10-21", ""), // a plain string that looks like a date
    ]);
    let d = "\"2026-10-20\"^^<http://www.w3.org/2001/XMLSchema#date>";
    assert_eq!(passing(&store, ">", d), vec!["2026-10-21"]);
    // Only the xsd:date passed: the plain string against an xsd:date is false.
    let both = passing(&store, ">", d).len() + passing(&store, "<=", d).len();
    assert_eq!(both, 2);
}

#[test]
fn plain_strings_keep_their_lexical_order() {
    let store = store_with(&[("apple", ""), ("pear", "")]);
    assert_eq!(passing(&store, ">", "\"banana\""), vec!["pear"]);
}
