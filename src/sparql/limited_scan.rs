//! Explicit LIMIT eligibility for lazy canonical triple deduplication.
use std::cell::Cell;

use spargebra::{
    algebra::{Expression, GraphPattern},
    term::{NamedNodePattern, TermPattern},
};

use super::{Bindings, TemporalContext};

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    #[cfg(test)]
    static ENTRIES: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(super) fn take_entries() -> usize {
    ENTRIES.with(|entries| entries.replace(0))
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

fn current(ctx: &TemporalContext, seed: &Bindings) -> bool {
    seed.is_empty()
        && ctx.valid_at.is_none()
        && ctx.as_of_tx.is_none()
        && !ctx.entails_rdfs
        && ctx.string_narrows.is_none()
}

/// Only an existence marker, not an arbitrary named-graph result prefix.
/// The constant projection cannot reject or multiply a leaf solution. Keep the
/// exact wrapper shape: no volatile expressions, filters, joins or graph vars.
pub(super) fn named_presence(
    pattern: &GraphPattern,
    ctx: &TemporalContext,
    seed: &Bindings,
) -> bool {
    if !current(ctx, seed) {
        return false;
    }
    let GraphPattern::Project { inner, variables } = pattern else {
        return false;
    };
    let GraphPattern::Extend {
        inner,
        variable,
        expression: Expression::Literal(_),
    } = inner.as_ref()
    else {
        return false;
    };
    if variables.as_slice() != [variable.clone()] {
        return false;
    }
    let GraphPattern::Graph {
        name: NamedNodePattern::NamedNode(_),
        inner,
    } = inner.as_ref()
    else {
        return false;
    };
    let GraphPattern::Bgp { patterns } = inner.as_ref() else {
        return false;
    };
    let [triple] = patterns.as_slice() else {
        return false;
    };
    let (TermPattern::Variable(subject), TermPattern::Variable(object)) =
        (&triple.subject, &triple.object)
    else {
        return false;
    };
    if subject == object || variable == subject || variable == object {
        return false;
    }
    match &triple.predicate {
        NamedNodePattern::Variable(predicate) => {
            predicate != subject && predicate != object && predicate != variable
        }
        // Type leaves have a separate subclass evaluator; retain its path.
        NamedNodePattern::NamedNode(predicate) => predicate.as_str() != crate::namespace::RDF_TYPE,
    }
}

pub(super) fn eligible(pattern: &GraphPattern, ctx: &TemporalContext, seed: &Bindings) -> bool {
    current(ctx, seed) && (single_independent_leaf(pattern) || named_presence(pattern, ctx, seed))
}

/// Evaluation is synchronous. Restore nested scopes on both errors and unwind.
pub(super) fn scoped<T>(enabled: bool, evaluate: impl FnOnce() -> T) -> T {
    #[cfg(test)]
    if enabled {
        ENTRIES.with(|entries| entries.set(entries.get() + 1));
    }
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.with(|value| value.set(self.0));
        }
    }
    let _reset = Reset(ACTIVE.with(|value| value.replace(enabled)));
    evaluate()
}
