//! Comparison operands read without copying (aegis-pu7).
//!
//! A FILTER comparison such as `?status = 'open'` used to clone the bound
//! value out of the row and build the literal afresh on every row; on the 2fa
//! stats aggregate that allocation was most of the expression time. An
//! [`Operand`] borrows a variable from the row and shares a memoized literal
//! or constant cast, so the comparison sees exactly the values
//! [`super::eval_expr`] returns, without the copies.

use std::rc::Rc;

use spargebra::algebra::{Expression, Function};

use super::{Bindings, cast_eval, eval_expr, eval_memo, literal_to_value};
use crate::store::Store;
use crate::types::Value;

/// A value as [`super::eval_expr`] would return it.
pub(super) enum Operand<'r> {
    Row(&'r Value),
    Shared(Rc<Value>),
    Owned(Value),
}

impl std::ops::Deref for Operand<'_> {
    type Target = Value;

    fn deref(&self) -> &Value {
        match self {
            Self::Row(value) => value,
            Self::Shared(value) => value,
            Self::Owned(value) => value,
        }
    }
}

fn from_shared<'r>(shared: eval_memo::Shared) -> Option<Operand<'r>> {
    match shared {
        eval_memo::Shared::Memo(value) => value.map(Operand::Shared),
        eval_memo::Shared::Computed(value) => value.map(Operand::Owned),
    }
}

/// [`super::eval_expr`] of `expr`, borrowed or shared where it can be.
pub(super) fn operand<'r>(
    store: &Store,
    expr: &Expression,
    row: &'r Bindings,
) -> Option<Operand<'r>> {
    match expr {
        Expression::Variable(var) => row.get(var.as_str()).map(Operand::Row),
        Expression::Literal(lit) => from_shared(eval_memo::shared_constant(expr, || {
            Some(literal_to_value(lit))
        })),
        Expression::FunctionCall(Function::Custom(function), args)
            if crate::sparql::casts::is_cast(function.as_str()) =>
        {
            match cast_eval::shared(store, expr, function.as_str(), args, row) {
                Some(shared) => from_shared(shared),
                None => eval_expr(store, expr, row).map(Operand::Owned),
            }
        }
        _ => eval_expr(store, expr, row).map(Operand::Owned),
    }
}

/// Shared value semantics for interpreted and slot-indexed comparisons.
pub(in crate::sparql) fn equal(a: &Value, b: &Value) -> Option<bool> {
    if a == b {
        return Some(true);
    }
    if let Some(order) = super::temporal_equality_order(a, b) {
        return order.map(|o| o == std::cmp::Ordering::Equal);
    }
    Some(crate::numeric_value::equal(a, b))
}

pub(in crate::sparql) fn compare(
    a: &Value,
    b: &Value,
    pred: impl Fn(std::cmp::Ordering) -> bool,
) -> Option<bool> {
    if a.datatype()
        .is_some_and(crate::namespace::is_numeric_datatype)
        && b.datatype()
            .is_some_and(crate::namespace::is_numeric_datatype)
    {
        return Some(crate::numeric_value::compare(a, b).is_some_and(pred));
    }
    // Temporal literals compare by VALUE (aegis-jd2t): offsets normalised, an
    // ill-typed lexeme, a mixed pair or an indeterminate (timezoned against
    // untimezoned) comparison is the SPARQL type error (`None`), so it fails
    // under `!` as well (aegis-u93). A lexical compare ordered 06:00-05:00
    // before 10:00Z and let "not-a-time" pass.
    if let Some(order) = super::temporal_order(a, b) {
        return order.map(pred);
    }
    // String comparison uses the LEXICAL form, so "hello"@en compares as
    // "hello" (it used to compare as "hello@en").
    Some(match (a.as_lexical(), b.as_lexical()) {
        (Some(a), Some(b)) => pred(a.cmp(b)),
        _ => false,
    })
}
