//! Controlled enforcement mutants for the scoped-read proofs.
//!
//! The design requires every enforcement point to have a paired negative that
//! DETECTS the disclosure when that point is disabled. Each [`Mutant`] names
//! one point. A test switches exactly one on for the current thread and checks
//! that the paired negative now observes data it must not see.
//!
//! The switch exists only under `cfg(test)`. In every other build, [`active`]
//! is a constant `false` that the compiler removes, so no configuration,
//! environment variable or request can reach it.

/// One enforcement point that a test may disable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mutant {
    /// `authorize`: grants for another principal are not excluded.
    PrincipalMatch,
    /// `authorize`: grants for another operation are not excluded.
    OperationMatch,
    /// `authorize`: revoked grants are not excluded.
    RevokedGrant,
    /// `authorize`: grants outside their validity window are not excluded.
    GrantValidity,
    /// `authorize`: an explicit dataset need not be a subset of the grant.
    DatasetSubset,
    /// `authorize`: mere overlap stands in for full dataset permission.
    DatasetIntersection,
    /// `query_response`: `FROM` / `FROM NAMED` outside the grant are not refused.
    QueryDatasetClause,
    /// `query_response`: `GRAPH <iri>` outside the grant is not refused.
    QueryGraphScope,
    /// `open_attached_cut`: trust a file's declared term space without
    /// measuring its real id range (aegis-n0wc.5 R1c).
    #[cfg_attr(not(test), allow(dead_code))]
    AttachTrustDeclaredSpace,
    /// Composition pool: returning a checkout skips DETACH (n0wc.5 P2).
    #[cfg_attr(not(test), allow(dead_code))]
    CompositionSkipDetach,
    /// Composition pool: checkout skips the clean-connection check (n0wc.5 P1).
    #[cfg_attr(not(test), allow(dead_code))]
    CompositionSkipCheckoutVerify,
}

#[cfg(test)]
thread_local! {
    static ACTIVE: std::cell::Cell<Option<Mutant>> = const { std::cell::Cell::new(None) };
}

/// True only inside [`with`] for the same mutant, on the same thread.
#[inline(always)]
pub(crate) fn active(mutant: Mutant) -> bool {
    #[cfg(test)]
    {
        ACTIVE.with(|current| current.get() == Some(mutant))
    }
    #[cfg(not(test))]
    {
        let _ = mutant;
        false
    }
}

/// Run `f` with exactly one enforcement point disabled on this thread.
/// The switch is cleared on return and on unwind.
#[cfg(test)]
pub(crate) fn with<R>(mutant: Mutant, f: impl FnOnce() -> R) -> R {
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            ACTIVE.with(|current| current.set(None));
        }
    }
    ACTIVE.with(|current| {
        assert!(current.get().is_none(), "mutants do not nest");
        current.set(Some(mutant));
    });
    let _clear = Clear;
    f()
}
