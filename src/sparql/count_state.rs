//! Bounded COUNT and paired metadata AVG accumulators; no input rows retained.
use crate::types::Value;
use spargebra::algebra::{AggregateExpression, AggregateFunction, Expression, Function};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Counter {
    pub(super) count: i64,
    /// DISTINCT values by [`super::count_key::distinct_hash`].
    seen: HashMap<u64, Vec<Value>>,
    hasher: foldhash::fast::RandomState,
    sum: f64,
    invalid: bool,
    double: bool,
}
impl Counter {
    fn accept(&mut self, value: Value, distinct: bool) -> bool {
        if distinct {
            let key = super::count_key::distinct_hash(&value, &self.hasher);
            let bucket = self.seen.entry(key).or_default();
            if bucket
                .iter()
                .any(|seen| super::count_key::distinct_same(seen, &value) && *seen == value)
            {
                return false;
            }
            bucket.push(value);
        }
        self.count += 1;
        true
    }
    pub(super) fn add(&mut self, value: Value, distinct: bool) {
        self.accept(value, distinct);
    }
    pub(super) fn average(&mut self, value: Value, distinct: bool) {
        let number = value.as_f64();
        let double = value.datatype() == Some(crate::namespace::XSD_DOUBLE);
        if self.accept(value, distinct) {
            if let Some(number) = number {
                self.sum += number;
                self.double |= double;
            } else {
                self.invalid = true;
            }
        }
    }
    pub(super) fn result(&self, expression: &AggregateExpression) -> Option<Value> {
        if matches!(
            expression,
            AggregateExpression::FunctionCall {
                name: AggregateFunction::Avg,
                ..
            }
        ) {
            if self.invalid || self.count == 0 {
                return None;
            }
            Some(super::aggregate::typed_number(
                self.sum / self.count as f64,
                if self.double {
                    crate::namespace::XSD_DOUBLE
                } else {
                    crate::namespace::XSD_DECIMAL
                },
            ))
        } else {
            Some(Value::Int(self.count))
        }
    }
}

// Only the pure arithmetic/date projection used alongside COUNT in metadata.
// Volatile functions and other expressions retain the ordinary evaluator.
pub(super) fn stable(expr: &Expression) -> bool {
    match expr {
        Expression::Variable(_) | Expression::Literal(_) | Expression::NamedNode(_) => true,
        Expression::Add(a, b)
        | Expression::Subtract(a, b)
        | Expression::Multiply(a, b)
        | Expression::Divide(a, b)
        | Expression::Equal(a, b)
        | Expression::Less(a, b)
        | Expression::LessOrEqual(a, b)
        | Expression::Greater(a, b)
        | Expression::GreaterOrEqual(a, b)
        | Expression::And(a, b)
        | Expression::Or(a, b) => stable(a) && stable(b),
        Expression::UnaryPlus(a) | Expression::UnaryMinus(a) | Expression::Not(a) => stable(a),
        Expression::If(a, b, c) => stable(a) && stable(b) && stable(c),
        Expression::FunctionCall(
            Function::Str
            | Function::StrDt
            | Function::Year
            | Function::Month
            | Function::Day
            | Function::Hours
            | Function::Minutes
            | Function::Seconds
            | Function::Floor,
            args,
        ) => args.iter().all(stable),
        _ => false,
    }
}
