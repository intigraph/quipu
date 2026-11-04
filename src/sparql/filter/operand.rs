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
