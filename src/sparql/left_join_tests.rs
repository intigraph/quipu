//! The keyed `LeftJoin` against the retained nested loop (aegis-d8c): the same
//! rows, in the same ORDER, for every generated input.
use super::left_join::{key_vars, left_join, left_join_nested, probe_counts, reset_probe_counts};
use super::{Bindings, TemporalContext};
use crate::{Store, namespace, types::Value};
use proptest::prelude::*;
use spargebra::algebra::Expression;
use spargebra::term::Variable;

/// Literal forms that tempt a normalising key (aegis-d8c condition 1): an
/// integer and its non-canonical spellings, a plain string and xsd:string,
/// language tags differing only in case, typed vs plain, and IRIs.
fn pool() -> Vec<Value> {
    let typed = |lexical: &str, datatype: &str| Value::Typed {
        lexical: lexical.into(),
        datatype: datatype.into(),
    };
    vec![
        Value::Int(1),
        typed("01", namespace::XSD_INTEGER),
        typed("+1", namespace::XSD_INTEGER),
        Value::Str("a".into()),
        typed("a", namespace::XSD_STRING),
        Value::Lang {
            lexical: "x".into(),
            lang: "en".into(),
        },
        Value::Lang {
            lexical: "x".into(),
            lang: "EN".into(),
        },
        Value::Str("x".into()),
        Value::Ref(1),
        Value::Ref(2),
        Value::Float(1.0),
        Value::Bool(true),
        // Canonical spelling: equal to `Int(1)` across variants. Keep it last.
        typed("1", namespace::XSD_INTEGER),
    ]
}

const VARS: [&str; 5] = ["s", "a", "b", "c", "_provider"];

/// A row: each variable unbound (None) or bound to a pool value.
fn row_strategy() -> impl Strategy<Value = Bindings> {
    let n = pool().len();
    proptest::collection::vec(proptest::option::of(0..n), VARS.len()).prop_map(move |slots| {
        let pool = pool();
        VARS.iter()
            .zip(slots)
            .filter_map(|(v, slot)| slot.map(|i| ((*v).to_string(), pool[i].clone())))
            .collect()
    })
}

/// A right side whose rows usually all bind `?s` (the triple-pattern case) but
/// sometimes do not, as when the right side carries its own OPTIONAL.
fn right_strategy() -> impl Strategy<Value = Vec<Bindings>> {
    (
        proptest::collection::vec(row_strategy(), 0..12),
        any::<bool>(),
        0..4usize,
    )
        .prop_map(|(mut rows, force_s, s_value)| {
            if force_s {
                let pool = pool();
                for (i, r) in rows.iter_mut().enumerate() {
                    r.entry("s".into())
                        .or_insert_with(|| pool[(s_value + i) % 3].clone());
                }
            }
            rows
        })
}

fn expressions() -> Vec<Option<Expression>> {
    let var = |n: &str| Expression::Variable(Variable::new_unchecked(n));
    vec![
        None,
        Some(Expression::Bound(Variable::new_unchecked("c"))),
        Some(Expression::Not(Box::new(Expression::Bound(
            Variable::new_unchecked("b"),
        )))),
        Some(Expression::Equal(Box::new(var("a")), Box::new(var("c")))),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Condition 4: SEQUENCE equality, not just the multiset.
    #[test]
    fn keyed_equals_nested_in_order(
        left in proptest::collection::vec(row_strategy(), 0..12),
        right in right_strategy(),
        e in 0..4usize,
    ) {
        let store = Store::open_in_memory().unwrap();
        let ctx = TemporalContext::default();
        let expression = expressions().swap_remove(e);
        let keyed = left_join(&store, &ctx, &left, &right, expression.as_ref()).unwrap();
        let nested = left_join_nested(&store, &ctx, &left, &right, expression.as_ref()).unwrap();
        prop_assert_eq!(keyed, nested);
    }

    /// Condition 1: the bucket key is never FINER than binding equality.
    #[test]
    fn equal_values_share_a_term_key(i in 0..13usize, j in 0..13usize) {
        let pool = pool();
        if pool[i] == pool[j] {
            prop_assert_eq!(pool[i].term_key(), pool[j].term_key());
        }
    }
}

fn row(pairs: &[(&str, Value)]) -> Bindings {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

/// Control for condition 1: equality crosses variants (`Int(1)` equals the
/// canonical `"1"^^xsd:integer`), so a key on the raw variant would be too
/// fine, while non-canonical spellings stay distinct. `term_key` agrees with
/// both, so the generated pool exercises a real cross-variant match.
#[test]
fn equality_crosses_variants_and_term_key_follows() {
    let pool = pool();
    let canonical_typed = pool.last().unwrap();
    assert!(matches!(canonical_typed, Value::Typed { .. }));
    assert_eq!(pool[0], *canonical_typed);
    assert_eq!(pool[0].term_key(), canonical_typed.term_key());
    assert_ne!(pool[0], pool[1]);
}

/// Condition 3: a variable some right row leaves unbound never keys, and a
/// federation label never keys (condition 2).
#[test]
fn key_set_comes_from_the_rows() {
    let left = vec![row(&[("s", Value::Ref(1)), ("_provider", Value::Ref(9))])];
    let right = vec![
        row(&[
            ("s", Value::Ref(1)),
            ("o", Value::Ref(2)),
            ("_provider", Value::Ref(8)),
        ]),
        row(&[("o", Value::Ref(3)), ("_provider", Value::Ref(8))]),
    ];
    assert!(key_vars(&left, &right).is_empty());
    let right = vec![row(&[("s", Value::Ref(1)), ("_provider", Value::Ref(8))])];
    assert_eq!(key_vars(&left, &right), vec!["s".to_string()]);
}

/// Condition 2: rows differing only in a federation label still join.
#[test]
fn federation_labels_do_not_split_buckets() {
    let store = Store::open_in_memory().unwrap();
    let ctx = TemporalContext::default();
    let left = vec![row(&[("s", Value::Ref(1)), ("_provider", Value::Ref(9))])];
    let right = vec![row(&[
        ("s", Value::Ref(1)),
        ("_provider", Value::Ref(8)),
        ("o", Value::Ref(2)),
    ])];
    let out = left_join(&store, &ctx, &left, &right, None).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["o"], Value::Ref(2));
}

/// Condition 6: an unbound-key left row takes the full scan and is counted;
/// a bound one probes.
#[test]
fn fallback_rows_are_counted() {
    let store = Store::open_in_memory().unwrap();
    let ctx = TemporalContext::default();
    let left = vec![
        row(&[("s", Value::Ref(1))]),
        row(&[("a", Value::Ref(5))]),
        row(&[("s", Value::Ref(2))]),
    ];
    let right = vec![row(&[("s", Value::Ref(1)), ("o", Value::Ref(7))])];
    reset_probe_counts();
    let keyed = left_join(&store, &ctx, &left, &right, None).unwrap();
    assert_eq!(probe_counts(), (1, 2));
    let nested = left_join_nested(&store, &ctx, &left, &right, None).unwrap();
    assert_eq!(keyed, nested);
}

/// Condition 5: a passed deadline still stops the keyed path.
#[test]
fn a_passed_deadline_still_times_out() {
    let store = Store::open_in_memory().unwrap();
    let ctx = TemporalContext {
        deadline: Some(crate::time::Deadline::after_millis(0)),
        ..TemporalContext::default()
    };
    let left = vec![row(&[("s", Value::Ref(1))])];
    let right = vec![row(&[("s", Value::Ref(1)), ("o", Value::Ref(2))])];
    let keyed = left_join(&store, &ctx, &left, &right, None);
    assert!(
        matches!(keyed, Err(crate::error::Error::QueryTimeout { .. })),
        "{keyed:?}"
    );
}

/// aegis-d8c condition 6, run by hand against a fixture store:
///
/// ```text
/// D8C_DB=<store.db> D8C_QUERY=<query.rq> cargo test --lib \
///   sparql::left_join_tests::fixture_probe_counts -- --ignored --nocapture
/// ```
///
/// Prints how many left rows probed a bucket and how many took the full scan.
#[test]
#[ignore = "needs a fixture store; run by hand"]
fn fixture_probe_counts() {
    let db = std::env::var("D8C_DB").expect("D8C_DB");
    let query = std::fs::read_to_string(std::env::var("D8C_QUERY").expect("D8C_QUERY")).unwrap();
    let store = Store::open(&db).unwrap();
    reset_probe_counts();
    let rows = super::query(&store, &query).unwrap().rows().len();
    let (fallback, probed) = probe_counts();
    println!("d8c fixture: rows={rows} probed_left_rows={probed} fallback_left_rows={fallback}");
}
