//! Private P1 commit seam, not startup or enrollment authority. A permit
//! holds the native lineage latch across the real SQL call. The SQLite hook
//! checks the same permit; it does not acquire and release a latch early.
//! No production caller pins a handle or enters this dormant seam yet.
#![allow(dead_code)]

use std::sync::{Arc, atomic::Ordering};

use super::{Store, ownership_fence::CommitFence, ownership_latch};
use crate::error::{Error, Result};

fn denied(why: &str) -> Error {
    Error::PolicyDenied(format!("ownership commit scope: {why}"))
}

pub(super) struct CommitPermit {
    fence: Arc<CommitFence>,
    _latch: Option<ownership_latch::Shared>,
}

impl CommitPermit {
    pub(super) fn acquire(fence: &Arc<CommitFence>) -> Result<Self> {
        fence
            .commit_scope
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| denied("a scope is already active"))?;
        let mut permit = Self {
            fence: Arc::clone(fence),
            _latch: None,
        };
        if let Some(path) = fence.journal_path() {
            permit._latch = Some(ownership_latch::shared(path)?);
            // Re-read under the shared latch, never before acquiring it.
            fence.check_journal()?;
        }
        if fence.revoked() {
            return Err(denied("this handle's admission was revoked"));
        }
        Ok(permit)
    }

    /// A private trusted caller supplies ONE real outer SQL completion. The
    /// permit remains live after SQLite's callback, until this call returns.
    pub(super) fn commit<T>(&self, sql: impl FnOnce() -> Result<T>) -> Result<T> {
        self.fence.commit_scope.store(2, Ordering::Release);
        let result = sql();
        #[cfg(test)]
        if let Some(pause) = self.fence.after_sql.lock().unwrap().as_ref() {
            pause();
        }
        result
    }
}

impl Drop for CommitPermit {
    fn drop(&mut self) {
        // The native shared latch is dropped AFTER the permit is invalidated.
        self.fence.commit_scope.store(0, Ordering::Release);
    }
}

/// Owns an outer savepoint and rolls it back on every unsuccessful exit,
/// including unwinding. The permit outlives cleanup and physical RELEASE.
struct WriteScope<'a> {
    store: &'a mut Store,
    permit: CommitPermit,
    finished: bool,
}

impl Drop for WriteScope<'_> {
    fn drop(&mut self) {
        if !self.finished {
            if !self.store.conn.is_autocommit()
                && self.store.conn.execute_batch("ROLLBACK").is_err()
            {
                self.permit.fence.revoke();
            }
            self.store.term_cache.borrow_mut().clear_persistent();
            self.store.invalidate_read_model();
            self.store.set_write_in_progress(false);
            self.store.policy_registry = None;
            #[cfg(feature = "owl")]
            {
                self.store.owl_cache = None;
            }
            self.store.clear_pending_write_events();
            self.store.pending_verdicts.clear();
            self.store.pending_requests.clear();
            self.store.pending_refusal = None;
            self.store.pending_quarantine = None;
        }
    }
}

impl Store {
    /// Frame ONE existing atomic Store operation (including registry writers
    /// that require their own BEGIN IMMEDIATE). This is not an all-or-nothing
    /// wrapper for a sequence of independently committing operations: that
    /// sequence must use `ownership_transaction` instead. No runtime caller.
    pub(super) fn ownership_operation<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        if !self.conn.is_autocommit() {
            return Err(denied("caller already owns an outer transaction"));
        }
        let fence = self
            .ownership_fence
            .as_ref()
            .ok_or_else(|| denied("a native disk fence is required"))?;
        let permit = CommitPermit::acquire(fence)?;
        let mut scope = WriteScope {
            store: self,
            permit,
            finished: false,
        };
        let value = scope.permit.commit(|| operation(scope.store))?;
        if !scope.store.conn.is_autocommit() {
            return Err(denied("operation left its outer transaction unfinished"));
        }
        scope.finished = true;
        Ok(value)
    }

    /// Private source-only transaction boundary. Savepoint-compatible Store
    /// methods can run inside its body; only this seam may commit the outer
    /// scope. Their returned receipts escape only after physical RELEASE.
    /// It neither pins a handle nor authorizes enrolled startup.
    pub(super) fn ownership_transaction<T>(
        &mut self,
        body: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        if !self.conn.is_autocommit() {
            return Err(denied("caller already owns an outer transaction"));
        }
        let fence = self
            .ownership_fence
            .as_ref()
            .ok_or_else(|| denied("a native disk fence is required"))?;
        let permit = CommitPermit::acquire(fence)?;
        self.conn.execute_batch("SAVEPOINT ownership_commit")?;
        let mut scope = WriteScope {
            store: self,
            permit,
            finished: false,
        };
        let value = body(scope.store)?;
        scope.permit.commit(|| {
            super::release::release(&scope.store.conn, "ownership_commit")?;
            Ok(())
        })?;
        if !scope.store.conn.is_autocommit() {
            return Err(denied("outer SQL completion was not established"));
        }
        scope.finished = true;
        Ok(value)
    }
}

#[cfg(test)]
#[path = "ownership_scope_tests.rs"]
mod tests;
