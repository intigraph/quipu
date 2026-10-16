//! Explicit LIMIT eligibility for lazy canonical triple deduplication.
use std::cell::Cell;

use spargebra::{
    algebra::GraphPattern,
    term::{NamedNodePattern, TermPattern},
};

use super::{Bindings, TemporalContext};

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn active() -> bool {
    ACTIVE.with(Cell::get)
}

fn single_independent_leaf(pattern: &GraphPattern) -> bool {
    match pattern {
        GraphPattern::Project { inner, .. } | GraphPattern::Reduced { inner } => {
            single_independent_leaf(inner)
        }
        GraphPattern::Bgp { patterns } if patterns.len() == 1 => {
            let tp = &patterns[0];
            matches!((&tp.subject, &tp.predicate, &tp.object),
                (TermPattern::Variable(s), NamedNodePattern::Variable(p), TermPattern::Variable(o))
                if s != p && s != o && p != o)
        }
        _ => false,
    }
}

pub(super) fn eligible(pattern: &GraphPattern, ctx: &TemporalContext, seed: &Bindings) -> bool {
    seed.is_empty()
        && ctx.valid_at.is_none()
        && ctx.as_of_tx.is_none()
        && !ctx.entails_rdfs
        && ctx.string_narrows.is_none()
        && single_independent_leaf(pattern)
}

/// Evaluation is synchronous. Restore nested scopes on both errors and unwind.
pub(super) fn scoped<T>(enabled: bool, evaluate: impl FnOnce() -> T) -> T {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.with(|value| value.set(self.0));
        }
    }
    let _reset = Reset(ACTIVE.with(|value| value.replace(enabled)));
    evaluate()
}
