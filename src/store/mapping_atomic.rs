//! C1's two-graph settlement boundary. No transport can call this primitive.
//! A record, its mapping projection and registry changes share one logical tx;
//! observers cannot see a tentative change that a later graph gate rolls back.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use super::{Datum, Store};
use crate::{Error, Result};

pub(super) const SOURCE: &str = "quipu:shape-mapping";

/// Preparation is performed under writer ownership, before allocating a tx.
/// An exact replay must return the original receipt without even a new tx row.
pub(super) enum Preparation<P, T> {
    Replay(T),
    Write(P),
}

impl Store {
    /// A batch may reuse only the exact transaction allocated by its outer
    /// mapping settlement, while the SQLite reservation remains active.
    pub(super) fn validate_mapping_transaction(
        &self,
        tx: i64,
        timestamp: &str,
        actor: Option<&str>,
        source: Option<&str>,
    ) -> Result<()> {
        let row = self
            .get_transaction(tx)?
            .ok_or_else(|| Error::InvalidValue("mapping transaction does not exist".into()))?;
        if self.conn.is_autocommit()
            || tx != self.latest_tx_id()?
            || row.timestamp != timestamp
            || row.actor.as_deref() != actor
            || row.source.as_deref() != source
        {
            return Err(Error::InvalidValue(
                "mapping transaction identity changed".into(),
            ));
        }
        Ok(())
    }

    /// The mapping writer alone constructs these closures from checked model
    /// data. Ordinary transactions and the existing multi-graph update API do
    /// not use this independent commit boundary.
    pub(super) fn mapping_atomic<P, T>(
        &mut self,
        actor: &str,
        prepare: impl FnOnce(&Self) -> Result<Preparation<P, T>>,
        write: impl FnOnce(&mut Self, P, &str, i64) -> Result<(T, Vec<(i64, Vec<Datum>)>)>,
    ) -> Result<T> {
        if !self.conn.is_autocommit() || self.write_in_progress() {
            return Err(Error::InvalidValue(
                "shape mapping requires an independent writer transaction".into(),
            ));
        }
        // This obtains SQLite writer ownership BEFORE the pin/replay/scope
        // reads. A deferred read transaction would permit stale preparation.
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        self.set_write_in_progress(true);
        let timestamp = crate::time::now_iso();
        let mut failed_graph = crate::schema::ROOT_GRAPH;
        let result = catch_unwind(AssertUnwindSafe(|| {
            crate::shape_mapping::write_scope::with_writer(|| {
                let plan = match prepare(self)? {
                    Preparation::Replay(value) => return Ok((value, None)),
                    Preparation::Write(plan) => plan,
                };
                let tx = crate::transaction_auth::begin(
                    &self.conn,
                    &timestamp,
                    Some(actor),
                    Some(SOURCE),
                )?;
                let (value, batches) = write(self, plan, &timestamp, tx)?;
                let mut graphs = BTreeSet::new();
                if batches.iter().any(|(g, _)| !graphs.insert(*g)) {
                    return Err(Error::InvalidValue(
                        "mapping settlement needs at most one batch per graph".into(),
                    ));
                }
                // The existing fact log's unique claim key is (e,a,v,tx), not
                // (g,e,a,v,tx). C1 projections use distinct record/pin identities;
                // refuse an accidental cross-graph alias rather than letting the
                // second insert silently update the first graph's claim.
                let mut claims = BTreeMap::new();
                for (graph, datums) in &batches {
                    for datum in datums {
                        let key = (datum.entity, datum.attribute, datum.value.to_bytes());
                        if let Some(prior) = claims.insert(key, *graph)
                            && prior != *graph
                        {
                            return Err(Error::InvalidValue(
                                "mapping projections alias a claim across graphs".into(),
                            ));
                        }
                    }
                }
                let mut staged = Vec::with_capacity(batches.len());
                for (graph, datums) in batches {
                    failed_graph = graph;
                    self.assert_write_target_allowed(graph, Some(SOURCE))?;
                    self.enforce_graph_authority(graph)
                        .map_err(|error| self.stash_refusal("authority", error, datums.len()))?;
                    let stage = self.stage_and_guard_in_transaction(
                        &datums,
                        &timestamp,
                        Some(actor),
                        Some(SOURCE),
                        graph,
                        None,
                        Some(tx),
                    )?;
                    staged.push((graph, datums, stage));
                }
                // Each graph's post-state check sees BOTH graph projections and
                // the registry mutation. Preserve graph-local SHACL cardinality.
                for (graph, datums, _) in &staged {
                    failed_graph = *graph;
                    self.enforce_shacl_poststate(datums, *graph, tx, true)
                        .map_err(|error| self.stash_refusal("shacl", error, datums.len()))?;
                }
                crate::promotion_control::claim_commit(Some(SOURCE))?;
                Ok((value, Some(staged)))
            })
        }));

        match result {
            Ok(Ok((value, None))) => {
                // Replay makes no durable change, including interned terms.
                self.mapping_rollback()?;
                Ok(value)
            }
            Ok(Ok((value, Some(staged)))) => {
                if let Err(error) = self.conn.execute_batch("COMMIT") {
                    self.mapping_rollback()?;
                    return Err(error.into());
                }
                self.set_write_in_progress(false);
                self.invalidate_read_model();
                // After a successful COMMIT, hook failure is indeterminate to
                // the caller. Do not roll back or retry an already landed tx.
                // Still notify all graph observers even if one hook fails.
                let mut hook_error = None;
                for (graph, datums, stage) in staged {
                    Self::observe_mapping_batch(datums.len(), &stage, actor, graph);
                    if let Err(error) =
                        self.after_commit_hooks(&datums, stage, &timestamp, Some(SOURCE), graph)
                    {
                        hook_error.get_or_insert(error);
                    }
                    if let Err(error) = self.invalidate_policy_registry_if_governance(&datums) {
                        hook_error.get_or_insert(error);
                    }
                }
                if let Some(provenance) = crate::write_provenance::current() {
                    crate::metrics::metrics().observe_write_provenance(&provenance);
                }
                self.flush_pending_verdicts(&timestamp, Some(actor));
                if let Some(error) = hook_error {
                    return Err(error);
                }
                Ok(value)
            }
            Ok(Err(error)) => {
                let refusal = self.pending_refusal.take();
                self.mapping_rollback()?;
                if let Some(refusal) = refusal {
                    self.record_refusal(
                        &refusal,
                        &self.graph_iri_of(failed_graph),
                        Some(actor),
                        Some(SOURCE),
                        &timestamp,
                    );
                }
                Err(error)
            }
            Err(panic) => {
                // Restore the worker/store before propagating a caller panic.
                // Never notify observers of the abandoned provisional state.
                let _ = self.mapping_rollback();
                resume_unwind(panic)
            }
        }
    }

    fn mapping_rollback(&mut self) -> Result<()> {
        let rollback = self.conn.execute_batch("ROLLBACK");
        self.set_write_in_progress(false);
        self.term_cache.borrow_mut().clear_persistent();
        self.invalidate_read_model();
        self.pending_refusal = None;
        self.pending_verdicts.clear();
        self.pending_requests.clear();
        self.clear_pending_write_events();
        rollback?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "mapping_atomic_tests.rs"]
mod tests;
