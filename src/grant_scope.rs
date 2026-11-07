//! Shared principal/graph/validity predicate. This holds no authority, locks,
//! operation grants, credential evidence, store pins or revocation barrier.
use std::collections::BTreeSet;

/// A borrowed view of one owner's exact grant. Callers retain their typed
/// operation checks and their independent policy/commit freshness checks.
pub struct GrantScope<'a> {
    pub principal: &'a str,
    pub graphs: &'a BTreeSet<String>,
    pub valid_from: u64,
    pub valid_until: u64,
    pub revoked: bool,
}

impl GrantScope<'_> {
    /// Every requested graph must be explicitly granted; an empty request is
    /// never authority, nor is a merely nonempty intersection. No globs, IRI
    /// normalization, implicit ROOT or fallback principal participate here.
    #[must_use]
    pub fn permits(&self, principal: &str, requested_graphs: &BTreeSet<String>, now: u64) -> bool {
        !self.revoked
            && self.principal == principal
            && self.valid_from <= now
            && now < self.valid_until
            && !requested_graphs.is_empty()
            && requested_graphs.is_subset(self.graphs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_principal_nonempty_subset_and_no_implicit_root() {
        let graphs = BTreeSet::from(["urn:g:a".into(), "urn:g:b".into()]);
        let scope = GrantScope {
            principal: "urn:principal",
            graphs: &graphs,
            valid_from: 10,
            valid_until: 20,
            revoked: false,
        };
        assert!(scope.permits("urn:principal", &graphs, 10));
        assert!(scope.permits("urn:principal", &BTreeSet::from(["urn:g:a".into()]), 19));
        assert!(!scope.permits("urn:other", &graphs, 10));
        assert!(!scope.permits("urn:principal", &BTreeSet::new(), 10));
        assert!(!scope.permits(
            "urn:principal",
            &BTreeSet::from(["urn:g:a".into(), "urn:g:foreign".into()]),
            10
        ));
        assert!(!scope.permits(
            "urn:principal",
            &BTreeSet::from(["urn:quipu:graph:root".into()]),
            10
        ));
        assert!(!scope.permits("urn:principal", &BTreeSet::from(["urn:g:*".into()]), 10));
    }

    #[test]
    fn half_open_time_window_and_revocation() {
        let graphs = BTreeSet::from(["urn:g:a".into()]);
        let mut scope = GrantScope {
            principal: "urn:principal",
            graphs: &graphs,
            valid_from: 10,
            valid_until: 20,
            revoked: false,
        };
        assert!(!scope.permits("urn:principal", &graphs, 9));
        assert!(scope.permits("urn:principal", &graphs, 10));
        assert!(scope.permits("urn:principal", &graphs, 19));
        assert!(!scope.permits("urn:principal", &graphs, 20));
        scope.revoked = true;
        assert!(!scope.permits("urn:principal", &graphs, 10));
        scope.revoked = false;
        scope.valid_until = scope.valid_from;
        assert!(!scope.permits("urn:principal", &graphs, 10));
    }
}
