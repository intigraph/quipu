//! UNION evaluation, split from the algebra size ratchet.

use spargebra::algebra::GraphPattern;

use super::{Bindings, TemporalContext, pattern::eval_pattern_seeded};
use crate::{error::Result, store::Store};

pub(super) fn evaluate(
    store: &Store,
    left: &GraphPattern,
    right: &GraphPattern,
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<(Vec<Bindings>, Vec<String>)> {
    let (mut left_rows, left_vars) = eval_pattern_seeded(store, left, ctx, seed)?;
    #[cfg(test)]
    super::read_snapshot::after_union_left();
    let (right_rows, right_vars) = eval_pattern_seeded(store, right, ctx, seed)?;
    left_rows.extend(right_rows);
    let mut vars = left_vars;
    for variable in right_vars {
        if !vars.contains(&variable) {
            vars.push(variable);
        }
    }
    Ok((left_rows, vars))
}
