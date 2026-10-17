//! Aggregate expression evaluation (COUNT, SUM, AVG, MIN, MAX, SAMPLE, `GROUP_CONCAT`).

use spargebra::algebra::{AggregateExpression, AggregateFunction};

use crate::store::Store;
use crate::types::Value;

use super::Bindings;
use super::filter::eval_expr;

/// Evaluate an aggregate expression over a group of rows.
pub fn eval_aggregate(
    store: &Store,
    agg: &AggregateExpression,
    rows: &[Bindings],
) -> Option<Value> {
    match agg {
        AggregateExpression::CountSolutions { distinct } => {
            if *distinct {
                let mut seen: Vec<&Bindings> = Vec::new();
                let count = rows
                    .iter()
                    .filter(|r| {
                        if seen.contains(r) {
                            false
                        } else {
                            seen.push(r);
                            true
                        }
                    })
                    .count();
                Some(Value::Int(count as i64))
            } else {
                Some(Value::Int(rows.len() as i64))
            }
        }
        AggregateExpression::FunctionCall {
            name,
            expr,
            distinct,
        } => {
            let mut values: Vec<Value> = rows
                .iter()
                .filter_map(|row| eval_expr(store, expr, row))
                .collect();
            if *distinct {
                values = distinct_values(values);
            }
            match name {
                AggregateFunction::Count => Some(Value::Int(values.len() as i64)),
                AggregateFunction::Sum => {
                    if let Some(exact) = values
                        .iter()
                        .map(crate::numeric_value::decimal)
                        .collect::<Option<Vec<_>>>()
                    {
                        let datatype = if values.iter().all(is_integral) {
                            crate::namespace::XSD_INTEGER
                        } else {
                            crate::namespace::XSD_DECIMAL
                        };
                        return Some(crate::numeric_value::decimal_result(
                            &exact.into_iter().sum(),
                            datatype,
                        ));
                    }
                    if values.iter().any(|value| value.as_f64().is_none()) {
                        return None;
                    }
                    let mut sum = 0.0f64;
                    let mut all_int = true;
                    let mut any_double = false;
                    for v in &values {
                        // as_f64 also sees numeric Typed literals (xsd:long,
                        // xsd:decimal, …), which keep their datatype rather
                        // than collapsing into Int/Float at parse (aegis-fmyi).
                        if let Some(n) = v.as_f64() {
                            sum += n;
                            all_int &= is_integral(v);
                            any_double |= v.datatype() == Some(crate::namespace::XSD_DOUBLE);
                        }
                    }
                    if all_int {
                        Some(Value::Int(sum as i64))
                    } else if any_double {
                        Some(typed_number(sum, crate::namespace::XSD_DOUBLE))
                    } else {
                        Some(typed_number(sum, crate::namespace::XSD_DECIMAL))
                    }
                }
                AggregateFunction::Avg => {
                    if !values.is_empty()
                        && let Some(exact) = values
                            .iter()
                            .map(crate::numeric_value::decimal)
                            .collect::<Option<Vec<_>>>()
                    {
                        let sum: bigdecimal::BigDecimal = exact.into_iter().sum();
                        return Some(crate::numeric_value::decimal_result(
                            &(sum
                                / bigdecimal::BigDecimal::from(u64::try_from(values.len()).ok()?)),
                            crate::namespace::XSD_DECIMAL,
                        ));
                    }
                    if values.is_empty() {
                        return None;
                    }
                    if values.iter().any(|value| value.as_f64().is_none()) {
                        return None;
                    }
                    let mut sum = 0.0f64;
                    let mut count = 0usize;
                    let mut any_double = false;
                    for v in &values {
                        if let Some(n) = v.as_f64() {
                            sum += n;
                            count += 1;
                            any_double |= v.datatype() == Some(crate::namespace::XSD_DOUBLE);
                        }
                    }
                    if count == 0 {
                        None
                    } else {
                        Some(typed_number(
                            sum / count as f64,
                            if any_double {
                                crate::namespace::XSD_DOUBLE
                            } else {
                                crate::namespace::XSD_DECIMAL
                            },
                        ))
                    }
                }
                AggregateFunction::Min => comparable_values(&values).then(|| {
                    values
                        .into_iter()
                        .reduce(|a, b| {
                            if compare_option_values(&Some(a.clone()), &Some(b.clone()))
                                == std::cmp::Ordering::Less
                            {
                                a
                            } else {
                                b
                            }
                        })
                        .unwrap_or(Value::Int(0))
                }),
                AggregateFunction::Max => comparable_values(&values).then(|| {
                    values
                        .into_iter()
                        .reduce(|a, b| {
                            if compare_option_values(&Some(a.clone()), &Some(b.clone()))
                                == std::cmp::Ordering::Greater
                            {
                                a
                            } else {
                                b
                            }
                        })
                        .unwrap_or(Value::Int(0))
                }),
                AggregateFunction::Sample => values.into_iter().next(),
                AggregateFunction::GroupConcat { separator } => {
                    let sep = separator.as_deref().unwrap_or(" ");
                    let strs: Vec<String> = values
                        .iter()
                        .map(|v| match v {
                            Value::Str(s) => s.clone(),
                            // GROUP_CONCAT concatenates lexical forms, so a
                            // lang literal contributes "hello", not "hello@en".
                            Value::Lang { lexical, .. } | Value::Typed { lexical, .. } => {
                                lexical.clone()
                            }
                            Value::Int(n) => n.to_string(),
                            Value::Float(f) => f.to_string(),
                            Value::Bool(b) => b.to_string(),
                            _ => String::new(),
                        })
                        .collect();
                    Some(Value::Str(strs.join(sep)))
                }
                AggregateFunction::Custom(_) => None,
            }
        }
    }
}

// Reference equality is exactly ID equality. Preserve first-occurrence order
// for SAMPLE/GROUP_CONCAT while avoiding a quadratic scan of subject groups.
// Mixed values retain Value::PartialEq, including cross-representation aliases.
fn distinct_values(mut values: Vec<Value>) -> Vec<Value> {
    if values.iter().all(|value| matches!(value, Value::Ref(_))) {
        let mut seen = std::collections::HashSet::new();
        values.retain(|value| match value {
            Value::Ref(id) => seen.insert(*id),
            _ => unreachable!("reference-only values checked above"),
        });
        return values;
    }
    let mut deduped = Vec::new();
    for value in values {
        if !deduped.contains(&value) {
            deduped.push(value);
        }
    }
    deduped
}

#[cfg(test)]
mod distinct_controls {
    use super::*;

    #[test]
    fn references_preserve_first_occurrence_order() {
        assert_eq!(
            distinct_values(vec![
                Value::Ref(9),
                Value::Ref(2),
                Value::Ref(9),
                Value::Ref(1)
            ]),
            vec![Value::Ref(9), Value::Ref(2), Value::Ref(1)]
        );
        assert!(distinct_values(Vec::new()).is_empty());
    }

    #[test]
    fn mixed_values_preserve_the_existing_equality_oracle() {
        let values = vec![
            Value::Ref(9),
            Value::Ref(9),
            Value::Int(1),
            Value::Typed {
                lexical: "1".into(),
                datatype: crate::namespace::XSD_INTEGER.into(),
            },
            Value::Str("1".into()),
            Value::Bool(true),
            Value::Int(1),
            Value::Ref(2),
        ];
        for shift in 0..values.len() {
            let mut input = values.clone();
            input.rotate_left(shift);
            let mut oracle = Vec::new();
            for value in input.iter().cloned() {
                if !oracle.contains(&value) {
                    oracle.push(value);
                }
            }
            assert_eq!(distinct_values(input), oracle);
        }
    }
}

fn comparable_values(values: &[Value]) -> bool {
    values.is_empty()
        || values.iter().all(|value| value.as_f64().is_some())
        || values.iter().all(|value| value.as_lexical().is_some())
}

pub(super) fn typed_number(value: f64, datatype: &str) -> Value {
    let lexical = if datatype == crate::namespace::XSD_DOUBLE {
        let rendered = format!("{value:E}");
        let (mantissa, exponent) = rendered.split_once('E').unwrap_or((&rendered, "0"));
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{mantissa}.0")
        };
        format!("{mantissa}E{}", exponent.parse::<i32>().unwrap_or(0))
    } else {
        let mut rendered = format!("{value:.12}");
        while rendered.ends_with('0') && !rendered.ends_with(".0") {
            rendered.pop();
        }
        rendered
    };
    Value::Typed {
        lexical,
        datatype: datatype.to_string(),
    }
}

/// Compare two optional Values for ordering (used by ORDER BY).
pub fn compare_option_values(a: &Option<Value>, b: &Option<Value>) -> std::cmp::Ordering {
    if let (Some(a), Some(b)) = (a, b)
        && a.datatype()
            .is_some_and(crate::namespace::is_numeric_datatype)
        && b.datatype()
            .is_some_and(crate::namespace::is_numeric_datatype)
    {
        return crate::numeric_value::compare(a, b).unwrap_or(std::cmp::Ordering::Equal);
    }
    match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(va), Some(vb)) => match (va, vb) {
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Str(a), Value::Str(b)) => a.cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::Ref(a), Value::Ref(b)) => a.cmp(b),
            _ => match (va.as_f64(), vb.as_f64()) {
                // Any numeric pair, including Typed numerics that kept their
                // datatype (xsd:long, xsd:decimal, …), orders numerically.
                (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
                // Otherwise fall back to lexical order for literals that have
                // a lexical form at all; anything else is incomparable.
                _ => match (va.as_lexical(), vb.as_lexical()) {
                    (Some(a), Some(b)) => a.cmp(b),
                    _ => std::cmp::Ordering::Equal,
                },
            },
        },
    }
}

/// Does this value carry an integer-valued datatype? Used by SUM to decide
/// whether the total stays an integer.
fn is_integral(v: &Value) -> bool {
    match v {
        Value::Int(_) => true,
        Value::Typed { datatype, .. } => matches!(
            datatype.as_str(),
            crate::namespace::XSD_INTEGER
                | crate::namespace::XSD_LONG
                | crate::namespace::XSD_INT
                | crate::namespace::XSD_SHORT
                | crate::namespace::XSD_BYTE
                | crate::namespace::XSD_NON_NEGATIVE_INTEGER
                | crate::namespace::XSD_POSITIVE_INTEGER
                | crate::namespace::XSD_UNSIGNED_LONG
                | crate::namespace::XSD_UNSIGNED_INT
        ),
        _ => false,
    }
}
