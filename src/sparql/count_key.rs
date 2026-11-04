//! Group and DISTINCT keys for the streaming count, without formatting
//! (aegis-pu7).
//!
//! The count used to bucket every row by `format!("{key:?}")` and every
//! DISTINCT value by `format!("{value:?}")`, then compare with `==` inside
//! the bucket. Formatting a key per row was a tenth of the 2fa stats
//! aggregate. These functions partition exactly as those Debug strings did:
//! [`same`] holds iff the derived Debug output is equal, and [`hash`] agrees
//! with it. Callers keep the `==` check, so which rows meet is unchanged.

use std::hash::{BuildHasher, Hash, Hasher};

use crate::types::Value;

/// Debug output equality. Derived Debug escapes strings and prints every
/// non-NaN float distinctly by bits, but every NaN as `NaN`.
fn float_same(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

/// Whether `format!("{a:?}") == format!("{b:?}")`.
pub(super) fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Ref(a), Value::Ref(b)) | (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => float_same(*a, *b),
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Bytes(a), Value::Bytes(b)) => a == b,
        (
            Value::Lang {
                lexical: a,
                lang: al,
            },
            Value::Lang {
                lexical: b,
                lang: bl,
            },
        ) => a == b && al == bl,
        (
            Value::Typed {
                lexical: a,
                datatype: ad,
            },
            Value::Typed {
                lexical: b,
                datatype: bd,
            },
        ) => a == b && ad == bd,
        _ => false,
    }
}

/// A hash that agrees with [`same`].
pub(super) fn hash<H: Hasher>(value: &Value, state: &mut H) {
    std::mem::discriminant(value).hash(state);
    match value {
        Value::Ref(n) | Value::Int(n) => n.hash(state),
        Value::Str(s) => s.hash(state),
        Value::Float(f) if f.is_nan() => {}
        Value::Float(f) => f.to_bits().hash(state),
        Value::Bool(b) => b.hash(state),
        Value::Bytes(b) => b.hash(state),
        Value::Lang { lexical, lang } => (lexical, lang).hash(state),
        Value::Typed { lexical, datatype } => (lexical, datatype).hash(state),
    }
}

/// [`same`] over a group key (unbound included).
pub(super) fn same_key(a: &[Option<Value>], b: &[Option<&Value>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => same(a, b),
            (None, None) => true,
            _ => false,
        })
}

/// [`hash`] over a group key.
pub(super) fn hash_key(key: &[Option<&Value>], build: &impl BuildHasher) -> u64 {
    let mut state = build.build_hasher();
    for value in key {
        match value {
            Some(value) => {
                1u8.hash(&mut state);
                hash(value, &mut state);
            }
            None => 0u8.hash(&mut state),
        }
    }
    state.finish()
}

/// The DISTINCT bucket identity: [`same`], except that `0.0` and `-0.0`
/// share one (the old key spelled both `Float(0.0)`).
pub(super) fn distinct_same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Float(a), Value::Float(b)) if *a == 0.0 && *b == 0.0 => true,
        _ => same(a, b),
    }
}

/// A hash that agrees with [`distinct_same`].
pub(super) fn distinct_hash(value: &Value, build: &impl BuildHasher) -> u64 {
    let mut state = build.build_hasher();
    match value {
        Value::Float(f) if *f == 0.0 => hash(&Value::Float(0.0), &mut state),
        _ => hash(value, &mut state),
    }
    state.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus() -> Vec<Value> {
        let typed = |lexical: &str, datatype: &str| Value::Typed {
            lexical: lexical.into(),
            datatype: datatype.into(),
        };
        vec![
            Value::Ref(1),
            Value::Ref(2),
            Value::Int(1),
            Value::Int(-1),
            typed("1", crate::namespace::XSD_INTEGER),
            typed("01", crate::namespace::XSD_INTEGER),
            typed("1", crate::namespace::XSD_DECIMAL),
            Value::Float(0.0),
            Value::Float(-0.0),
            Value::Float(1.0),
            Value::Float(f64::NAN),
            Value::Float(f64::from_bits(f64::NAN.to_bits() ^ 1)),
            Value::Float(-f64::NAN),
            Value::Float(f64::INFINITY),
            Value::Bool(true),
            Value::Bool(false),
            Value::Str(String::new()),
            Value::Str("a".into()),
            Value::Str("a\"b".into()),
            Value::Str("a\\\"b".into()),
            Value::Str("open".into()),
            Value::Lang {
                lexical: "open".into(),
                lang: "en".into(),
            },
            Value::Lang {
                lexical: "open".into(),
                lang: "EN".into(),
            },
            typed("open", "http://example.org/t"),
            Value::Bytes(b"open".to_vec()),
            Value::Bytes(Vec::new()),
        ]
    }

    /// The partition is exactly the old `format!("{v:?}")` one, and the
    /// hashes agree with it.
    #[test]
    fn same_is_debug_equality_and_hashes_agree() {
        let build = foldhash::fast::RandomState::default();
        let h = |v: &Value| {
            let mut state = build.build_hasher();
            hash(v, &mut state);
            state.finish()
        };
        let values = corpus();
        for a in &values {
            for b in &values {
                let debug = format!("{a:?}") == format!("{b:?}");
                assert_eq!(same(a, b), debug, "{a:?} vs {b:?}");
                if debug {
                    assert_eq!(h(a), h(b), "{a:?} vs {b:?}");
                }
                let old_key = |v: &Value| {
                    if matches!(v, Value::Float(n) if *n == 0.0) {
                        "Float(0.0)".to_string()
                    } else {
                        format!("{v:?}")
                    }
                };
                let distinct = old_key(a) == old_key(b);
                assert_eq!(distinct_same(a, b), distinct, "{a:?} vs {b:?}");
                if distinct {
                    assert_eq!(distinct_hash(a, &build), distinct_hash(b, &build));
                }
            }
        }
        // Group keys: unbound is its own value and positions matter.
        let one = Value::Int(1);
        let key = [Some(one.clone()), None];
        assert!(same_key(&key, &[Some(&one), None]));
        assert!(!same_key(&key, &[None, Some(&one)]));
        assert_eq!(
            format!("{key:?}") == format!("{:?}", [None, Some(&one)]),
            same_key(&key, &[None, Some(&one)])
        );
        assert_eq!(
            hash_key(&[Some(&one), None], &build),
            hash_key(&[Some(&Value::Int(1)), None], &build)
        );
    }
}
