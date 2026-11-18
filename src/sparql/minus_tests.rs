//! The hashed `MINUS` must remove exactly the rows the old nested loop removed.

use super::subtract;
use crate::Value;
use crate::sparql::pattern_util::merge_bindings;
use crate::sparql::{Bindings, TemporalContext};

/// The pre-aegis-49s evaluator, kept verbatim as the oracle.
fn nested_loop(left: &[Bindings], right: &[Bindings]) -> Vec<Bindings> {
    left.iter()
        .filter(|left_row| {
            !right.iter().any(|right_row| {
                let shared_domain = left_row.keys().any(|key| right_row.contains_key(key));
                shared_domain && merge_bindings(left_row, right_row).is_some()
            })
        })
        .cloned()
        .collect()
}

fn row(pairs: &[(&str, Value)]) -> Bindings {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

fn typed_int(lexical: &str) -> Value {
    Value::Typed {
        lexical: lexical.into(),
        datatype: crate::namespace::XSD_INTEGER.into(),
    }
}

fn same_as_oracle(left: &[Bindings], right: &[Bindings]) -> Vec<Bindings> {
    let expected = nested_loop(left, right);
    let actual = subtract(left.to_vec(), right, &TemporalContext::default()).unwrap();
    assert_eq!(actual, expected, "left={left:?} right={right:?}");
    actual
}

#[test]
fn removes_only_compatible_rows_sharing_a_variable() {
    let left = vec![
        row(&[("s", Value::Ref(1)), ("o", Value::Str("a".into()))]),
        row(&[("s", Value::Ref(2)), ("o", Value::Str("b".into()))]),
        row(&[("s", Value::Ref(3))]),
    ];
    let right = vec![row(&[("s", Value::Ref(2)), ("v", Value::Str("x".into()))])];
    let kept = same_as_oracle(&left, &right);
    assert_eq!(kept.len(), 2, "only s=2 is removed");
}

#[test]
fn disjoint_domains_never_remove() {
    // SPARQL 8.3: no shared variable means no exclusion, even though the rows
    // are trivially compatible.
    let left = vec![row(&[("s", Value::Ref(1))])];
    let right = vec![row(&[("x", Value::Ref(1))])];
    assert_eq!(same_as_oracle(&left, &right).len(), 1);
}

#[test]
fn partially_bound_rows_use_their_own_shared_domain() {
    // OPTIONAL leaves rows with different domains on both sides.
    let left = vec![
        row(&[("s", Value::Ref(1)), ("o", Value::Ref(9))]),
        row(&[("s", Value::Ref(1))]),
        row(&[("o", Value::Ref(9))]),
        row(&[("s", Value::Ref(2)), ("o", Value::Ref(8))]),
    ];
    let right = vec![
        row(&[("s", Value::Ref(1)), ("o", Value::Ref(7))]),
        row(&[("o", Value::Ref(8))]),
    ];
    same_as_oracle(&left, &right);
}

#[test]
fn term_identity_matches_value_equality() {
    // Int(1) and "1"^^xsd:integer are the same term; "01" is not; a Ref is
    // never equal to a string spelling of an IRI.
    let left = vec![
        row(&[("v", Value::Int(1))]),
        row(&[("v", typed_int("01"))]),
        row(&[("v", Value::Str("http://e/x".into()))]),
        row(&[("v", Value::Float(f64::NAN))]),
    ];
    let right = vec![
        row(&[("v", typed_int("1"))]),
        row(&[("v", Value::Ref(5))]),
        row(&[("v", Value::Float(f64::NAN))]),
    ];
    same_as_oracle(&left, &right);
}

#[test]
fn federation_metadata_counts_as_shared_but_never_conflicts() {
    let left = vec![
        row(&[("_provider", Value::Str("a".into())), ("s", Value::Ref(1))]),
        row(&[("_provider", Value::Str("a".into())), ("s", Value::Ref(2))]),
    ];
    let right = vec![
        // Shares only metadata (different provider): still excludes everything.
        row(&[("_provider", Value::Str("b".into()))]),
    ];
    assert!(same_as_oracle(&left, &right).is_empty());
    let right = vec![row(&[
        ("_provider", Value::Str("b".into())),
        ("s", Value::Ref(2)),
    ])];
    assert_eq!(same_as_oracle(&left, &right).len(), 1);
}

#[test]
fn order_and_duplicates_are_preserved() {
    let a = row(&[("s", Value::Ref(1))]);
    let b = row(&[("s", Value::Ref(2))]);
    let left = vec![b.clone(), a.clone(), b.clone(), a];
    let right = vec![row(&[("s", Value::Ref(3))])];
    assert_eq!(same_as_oracle(&left, &right), left);
    assert!(same_as_oracle(&left, &[]).len() == 4);
}

#[test]
fn exhaustive_small_domains_agree_with_the_oracle() {
    // Every row over keys {a, b, _trust} with values {unbound, 1, 2}.
    let values = [None, Some(Value::Ref(1)), Some(Value::Ref(2))];
    let mut rows = Vec::new();
    for a in &values {
        for b in &values {
            for t in [None, Some(Value::Str("t".into()))] {
                let mut r = Bindings::default();
                if let Some(v) = a {
                    r.insert("a".into(), v.clone());
                }
                if let Some(v) = b {
                    r.insert("b".into(), v.clone());
                }
                if let Some(v) = &t {
                    r.insert("_trust".into(), v.clone());
                }
                rows.push(r);
            }
        }
    }
    for split in 0..rows.len() {
        let (left, right) = rows.split_at(split);
        same_as_oracle(left, right);
        same_as_oracle(right, left);
    }
}

#[test]
fn large_minus_is_linear_not_quadratic() {
    // 200k x 20k used to be 4e9 merged-row tests. With the index it is one
    // probe per left row and group.
    let left: Vec<Bindings> = (0..200_000).map(|i| row(&[("s", Value::Ref(i))])).collect();
    let right: Vec<Bindings> = (0..20_000)
        .map(|i| row(&[("s", Value::Ref(i * 10)), ("x", Value::Int(i))]))
        .collect();
    let started = std::time::Instant::now();
    let kept = subtract(left, &right, &TemporalContext::default()).unwrap();
    assert_eq!(kept.len(), 180_000);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "MINUS took {:?}",
        started.elapsed()
    );
}
