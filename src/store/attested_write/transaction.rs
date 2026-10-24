//! The receipt, the consumed challenge, the counter and the written facts
//! share ONE writer savepoint. The normal staging/guard pipeline runs, but
//! observers wait until the commit is final, so none of them can see a write
//! that is later rolled back.

use super::*;

impl Store {
    pub(super) fn attest_atomic<T>(
        &mut self,
        body: impl FnOnce(&mut Self, &str) -> Result<(T, Option<(String, String, Vec<Datum>)>)>,
    ) -> Result<T> {
        // Refuse nesting: observers must not witness a tentative write inside
        // speculate() or an unrelated caller transaction.
        if !self.conn.is_autocommit() {
            return Err(refuse("requires an independent writer transaction"));
        }
        self.conn.execute_batch("SAVEPOINT quipu_attest")?;
        let ts = crate::time::now_iso();
        self.set_write_in_progress(true);
        // Set once a write reaches the graph gates, so a refusal there keeps
        // its normal audit (refusal event and denial verdicts) after rollback.
        let mut audit: Option<(i64, String)> = None;
        let result = (|| {
            // Acquire SQLite writer ownership BEFORE any state is read.
            self.conn
                .execute("UPDATE attest_switch SET enabled=enabled WHERE id=1", [])?;
            let (value, write) = body(self, &ts)?;
            let Some((graph_iri, actor, datums)) = write else {
                return Ok((value, None));
            };
            let graph = self.graph_create(&graph_iri)?;
            self.assert_graph_is_writable(graph)?;
            audit = Some((graph, actor.clone()));
            self.enforce_graph_authority(graph)?;
            let staged =
                self.stage_and_guard(&datums, &ts, Some(&actor), Some(SOURCE), graph, None)?;
            let tx = self.latest_tx_id()?;
            self.conn
                .execute("UPDATE attest_receipts SET tx=?1 WHERE tx IS NULL", [tx])?;
            Ok((value, Some((datums, staged, graph))))
        })();
        match result {
            Ok((value, written)) => {
                if let Err(error) = self.conn.execute_batch("RELEASE quipu_attest") {
                    let _ = self
                        .conn
                        .execute_batch("ROLLBACK TO quipu_attest; RELEASE quipu_attest");
                    self.set_write_in_progress(false);
                    self.term_cache.borrow_mut().clear_persistent();
                    return Err(error.into());
                }
                self.set_write_in_progress(false);
                if let Some((datums, staged, graph)) = written {
                    self.invalidate_read_model();
                    // A post-commit hook error is indeterminate to the caller;
                    // the receipt lookup recovers the committed write.
                    self.after_commit_hooks(&datums, staged, &ts, Some(SOURCE), graph)?;
                }
                Ok(value)
            }
            Err(error) => {
                let rollback = self
                    .conn
                    .execute_batch("ROLLBACK TO quipu_attest; RELEASE quipu_attest");
                self.set_write_in_progress(false);
                self.term_cache.borrow_mut().clear_persistent();
                // Zero accepted mutation, but the refusal is still audited,
                // exactly as the ordinary write path does after its rollback.
                let refusal = self.pending_refusal.take();
                self.clear_pending_write_events();
                self.pending_requests.clear();
                if let Some((graph, actor)) = audit {
                    self.flush_pending_verdicts(&ts, Some(&actor));
                    if let Some(r) = refusal {
                        let iri = self.graph_iri_of(graph);
                        self.record_refusal(&r, &iri, Some(&actor), Some(SOURCE), &ts);
                    }
                } else {
                    self.pending_verdicts.clear();
                }
                rollback?;
                Err(error)
            }
        }
    }
}
