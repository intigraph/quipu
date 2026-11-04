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
    let cast = || {
        crate::sparql::casts::cast(
            store,
            function,
            eval_expr(store, args.first()?, row)?,
            canonical_double,
            format_decimal,
        )
    };
    if matches!(args, [Expression::Literal(_)]) {
        eval_memo::constant(expr, cast)
    } else {
        cast()
    }
}
