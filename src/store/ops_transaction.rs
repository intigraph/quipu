//! Normal transactional admission, staging, rollback and commit.
use super::*;

impl Store {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn transact_to_graph_scoped_checked(
        &mut self,
        datums: &[Datum],
        timestamp: &str,
        actor: Option<&str>,
        source: Option<&str>,
        graph: i64,
        retract_source: Option<&str>,
        expected: &[crate::store::conditional::ExpectedValue],
        set: Option<&crate::store::conditional::SetExpectation>,
    ) -> Result<i64> {
        self.transact_to_graph_window_checked(
            datums,
            timestamp,
            actor,
            source,
            graph,
            retract_source,
            expected,
            set,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::store) fn transact_to_graph_window_checked(
        &mut self,
        datums: &[Datum],
        timestamp: &str,
        actor: Option<&str>,
        source: Option<&str>,
        graph: i64,
        retract_source: Option<&str>,
        expected: &[crate::store::conditional::ExpectedValue],
        set: Option<&crate::store::conditional::SetExpectation>,
        window: Option<crate::store::source_claims::ClaimWindow>,
        window_guard: Option<&crate::store::source_claims::WindowGuard>,
    ) -> Result<i64> {
        if let Some(window) = window {
            if window.after < 0
                || window.through <= window.after
                || retract_source.is_none()
                || datums.iter().any(|d| d.op != Op::Retract)
            {
                return Err(crate::Error::InvalidValue(
                    "invalid retract-only claim window".into(),
                ));
            }
        }
        // Ownership is revalidated on this handle, before any gate can write an
        // audit row: a handle opened earlier must not outlive invalid ownership.
        self.ensure_ownership_writable()?;
        // Normal graph/authority gates precede the savepoint. Conditional
        // callers share this exact pipeline and its post-rollback evidence.
        self.assert_write_target_allowed(graph, source)?;
        // Authority precedes the savepoint: its refusal survives rollback.
        if let Err(e) = self.enforce_graph_authority(graph) {
            let refusal = crate::store::events::PendingRefusal {
                gate: "authority",
                reason: e.to_string(),
                refused_datums: datums.len(),
            };
            self.record_refusal(
                &refusal,
                &self.graph_iri_of(graph),
                actor,
                source,
                timestamp,
            );
            return Err(e);
        }
        // Suspended models cannot answer policy checks from stale pre-state.
        self.set_write_in_progress(true);
        self.conn.execute_batch("SAVEPOINT quipu_transact")?;
        let staged = self
            .check_expected(graph, expected, datums, set)
            .and_then(|()| {
                if window.is_some() {
                    // Serialize the deletion-floor pre-state with its staged
                    // mutation, including claims outside the selected window.
                    self.conn
                        .execute("UPDATE transactions SET id=id WHERE 0", [])?;
                    if let Some(guard) = window_guard {
                        self.check_window_guard(guard)?;
                    }
                    self.enforce_retraction_shapes_window(
                        datums,
                        graph,
                        retract_source,
                        actor,
                        source,
                        window,
                    )
                    .map_err(|e| self.stash_refusal("shacl", e, datums.len()))?;
                }
                self.stage_and_guard_window(
                    datums,
                    timestamp,
                    actor,
                    source,
                    graph,
                    retract_source,
                    window,
                )
            })
            .and_then(|staged| {
                self.enforce_shacl_poststate(datums, graph, staged.tx_id, !expected.is_empty())
                    .map_err(|e| self.stash_refusal("shacl", e, datums.len()))?;
                crate::promotion_control::claim_commit(source)?;
                Ok(staged)
            });
        match staged {
            Ok(mut staged) => {
                let tx_id = staged.tx_id;
                let counts = staged.counts;
                // Taken before `staged` moves into after_commit_hooks below.
                let effective = staged.effective.take();
                if let Err(error) = crate::store::release::release(&self.conn, "quipu_transact") {
                    // The commit itself was refused (e.g. by the ownership
                    // fence) and nothing landed: run the same cleanup as the
                    // error path, but record no verdicts or refusal for a
                    // write that passed its gates and was never committed.
                    self.set_write_in_progress(false);
                    self.pending_refusal = None;
                    self.clear_pending_write_events();
                    self.pending_verdicts.clear();
                    self.pending_requests.clear();
                    self.pending_quarantine = None;
                    return Err(error.into());
                }
                self.after_commit_hooks(datums, staged, timestamp, source, graph)?;
                // A write that defined or amended a policy makes the cached
                // registry stale.
                self.invalidate_policy_registry_if_governance(datums)?;
                // Bring the resident model up to date: apply the change set when
                // the write vouched for it, drop when it could not (quipu-m9h).
                self.set_write_in_progress(false);
                self.maintain_read_model(graph, effective.as_deref(), tx_id);
                // Memory telemetry (memory telemetry): count the commit and sample RSS
                // so a burst-export spike is captured at the write that caused it.
                let kind = crate::write_kind::classify(actor, source);
                crate::metrics::metrics().observe_write(&crate::metrics::writes::WriteCounts {
                    submitted: datums.len() as u64,
                    inferred: counts.inferred as u64,
                    asserted: counts.asserted as u64,
                    retracted: counts.retracted as u64,
                    superseded: counts.superseded as u64,
                    root: graph == crate::schema::ROOT_GRAPH,
                    kind,
                });
                // Count the commit under the requesting client's declared
                // provenance (aegis-7zp4rc). The engine's own writers that a
                // request merely triggers are not that client's writes.
                if !matches!(
                    kind,
                    crate::write_kind::WriteKind::Verdict
                        | crate::write_kind::WriteKind::Reasoner
                        | crate::write_kind::WriteKind::Migration
                ) && let Some(p) = crate::write_provenance::current()
                {
                    crate::metrics::metrics().observe_write_provenance(&p);
                }
                // Q-VERDICT-PERSIST: outside the savepoint, so the accept case
                // and the denial case below record identically.
                self.flush_pending_verdicts(timestamp, actor);
                Ok(tx_id)
            }
            Err(e) => {
                // Undo the staged datums and the transaction row; the store is
                // left byte-identical to before this call (no partial mutation).
                let _ = crate::store::release::rollback_release(&self.conn, "quipu_transact");
                // The suspended model never observed rolled-back rows.
                self.set_write_in_progress(false);
                // Take the staged refusal BEFORE flushing verdicts: the flush
                // runs a nested transact whose own error path would otherwise
                // consume it under the verdict-writer's actor/source.
                let refusal = self.pending_refusal.take();
                // Aborted validation advisories must not ride the audit writer
                // below (or an unrelated later write).
                self.clear_pending_write_events();
                // AFTER the rollback, deliberately. The verdict of a denial is
                // the one worth keeping — an accepted write leaves its own
                // evidence in the facts it wrote, a refused one leaves nothing.
                self.flush_pending_verdicts(timestamp, actor);
                // Same reason, same ordering, for the refusal event
                // (camayoc-0d3): recorded only after the rollback succeeded,
                // on the same connection, and never able to mask `e` —
                // `record_refusal` swallows its own failures. Refusals under
                // speculate() are skipped inside (actor/source "speculate").
                if let Some(r) = refusal {
                    self.record_refusal(&r, &self.graph_iri_of(graph), actor, source, timestamp);
                }
                Err(e)
            }
        }
    }
}
