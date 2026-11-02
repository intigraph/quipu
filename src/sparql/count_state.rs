//! Bounded COUNT and paired metadata AVG accumulators; no input rows retained.
use crate::types::Value;
use spargebra::algebra::{AggregateExpression, AggregateFunction};
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
