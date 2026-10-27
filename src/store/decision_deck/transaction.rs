//! Protected state and projected facts share ONE writer savepoint. Run the
//! normal staging/guard pipeline, but postpone observers until settlement is
//! committed. A generic nested transact would notify observers before the
//! outer savepoint could fail and roll the decision back.

use super::*;

impl Store {
    pub(super) fn deck_atomic<T>(
        &mut self,
        actor: &str,
        body: impl FnOnce(&mut Self, &str) -> Result<(T, Vec<Datum>)>,
    ) -> Result<T> {
        // Refuse nesting: post-commit observers must not witness tentative
        // decisions inside speculate() or an unrelated caller transaction.
        if !self.conn.is_autocommit() {
            return Err(refuse("requires an independent writer transaction"));
        }
        self.ensure_ownership_writable()?;
        self.conn.execute_batch("SAVEPOINT quipu_deck")?;
        let ts = crate::time::now_iso();
        self.set_write_in_progress(true);
        let result = (|| {
            // Acquire SQLite writer ownership BEFORE scope/revision reads.
            self.conn
                .execute("UPDATE deck_config SET enabled=enabled WHERE id=1", [])?;
            let (value, datums) = body(self, &ts)?;
            let graph = self.graph_create(GRAPH)?;
            self.assert_graph_is_writable(graph)?;
            self.enforce_graph_authority(graph)?;
            let staged = self.stage_and_guard(
                &datums,
                &ts,
                Some(actor),
                Some("decision-deck"),
                graph,
                None,
            )?;
            let tx = self.latest_tx_id()?;
            self.conn
                .execute("UPDATE deck_receipts SET tx=?1 WHERE tx IS NULL", [tx])?;
            Ok((value, datums, staged, graph))
        })();
        match result {
            Ok((value, datums, staged, graph)) => {
                if let Err(error) = crate::store::release::release(&self.conn, "quipu_deck") {
                    let _ = crate::store::release::rollback_release(&self.conn, "quipu_deck");
                    self.set_write_in_progress(false);
                    self.term_cache.borrow_mut().clear_persistent();
                    return Err(error.into());
                }
                self.set_write_in_progress(false);
                self.invalidate_read_model();
                // A post-commit hook error is indeterminate to the caller;
                // exact receipt lookup recovers the already committed action.
                self.after_commit_hooks(&datums, staged, &ts, Some("decision-deck"), graph)?;
                Ok(value)
            }
            Err(error) => {
                let rollback = crate::store::release::rollback_release(&self.conn, "quipu_deck");
                self.set_write_in_progress(false);
                self.term_cache.borrow_mut().clear_persistent();
                self.pending_refusal = None;
                self.pending_verdicts.clear();
                self.pending_requests.clear();
                rollback?;
                Err(error)
            }
        }
    }
}
