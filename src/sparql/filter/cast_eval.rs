//! XSD constructor casts, with a literal argument computed once per
//! evaluation (aegis-xdvg.6).

use spargebra::algebra::Expression;

use super::{Bindings, canonical_double, eval_expr, eval_memo, format_decimal};
use crate::store::Store;
use crate::types::Value;

/// `function(args)` for a cast `function`. A cast of a literal is a
/// constant, so inside an aggregate's memo scope it is computed once.
pub(super) fn cast(
    store: &Store,
    expr: &Expression,
    function: &str,
    args: &[Expression],
    row: &Bindings,
) -> Option<Value> {
    if matches!(args, [Expression::Literal(_)]) {
        eval_memo::constant(expr, || compute(store, function, args, row))
    } else {
        compute(store, function, args, row)
    }
}

/// A cast of a literal, shared from the memo rather than copied out of it;
/// `None` for any other cast (aegis-pu7).
pub(super) fn shared(
    store: &Store,
    expr: &Expression,
    function: &str,
    args: &[Expression],
    row: &Bindings,
) -> Option<eval_memo::Shared> {
    matches!(args, [Expression::Literal(_)])
        .then(|| eval_memo::shared_constant(expr, || compute(store, function, args, row)))
}

fn compute(store: &Store, function: &str, args: &[Expression], row: &Bindings) -> Option<Value> {
    crate::sparql::casts::cast(
        store,
        function,
        eval_expr(store, args.first()?, row)?,
        canonical_double,
        format_decimal,
    )
}
