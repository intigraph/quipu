//! Keep a BGP whose every subject is already fixed off the whole-graph model.
//!
//! aegis-bqq6: `<s> seeds:status ?s . <s> seeds:priority ?p` in a seeds
//! project graph took 4.5-6.7 s while each pattern alone took 0.02 s. Every
//! BGP of two or more patterns went to the resident read model, whose build
//! or write catch-up scans every current fact in the graph, so a point read
//! paid for the whole graph. VALUES-seeded subjects hit the same path, and the
//! Brief per-card read `daily_brief_ask_item_v1` timed out at 10 s.
//!
//! When every pattern's subject is a constant IRI or a variable the seed
//! already binds, each pattern is a bound-subject index probe. The nested
//! loop then costs one probe per pattern per row and never reads a predicate
//! across the graph. Anything with a free subject keeps the existing planner.

use spargebra::term::{TermPattern, TriplePattern};

use super::Bindings;

/// Whether every pattern's subject is fixed before evaluation starts: a
/// constant IRI, or a variable the seed binds.
pub(super) fn all_subjects_fixed(patterns: &[TriplePattern], seed: &Bindings) -> bool {
    !patterns.is_empty()
        && patterns.iter().all(|pattern| {
            // An if-chain, not a match: TermPattern's variant set depends on
            // features, so a wildcard arm trips clippy on one build or another.
            if let TermPattern::Variable(v) = &pattern.subject {
                seed.contains_key(v.as_str())
            } else {
                matches!(pattern.subject, TermPattern::NamedNode(_))
            }
        })
}

#[cfg(test)]
#[path = "bound_subject_bgp_tests.rs"]
mod tests;
