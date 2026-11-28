//! Durable admission, effect and receipt stay in the selected owner. This is
//! a native boundary, not an HTTP route or production startup initializer.
use super::*;
use crate::store::knot_completion::Binding;
use sha2::{Digest, Sha256};

impl Owner {
    fn completion_context(&self) -> String {
        let context = serde_json::json!({
            "contract": "quipu.graph-owner-knot/v1",
            "catalog_revision": self.catalog.revision(),
            "graph": self.route.graph,
            "file_id": self.route.file_id,
            "store_id": self.route.store_id,
            "log_epoch": self.route.log_epoch,
            "generation": self.route.generation,
            "policy": self.route.policy,
        });
        format!("{:x}", Sha256::digest(context.to_string().as_bytes()))
    }

    fn check_durable_request(&self, request: &WriteRequest<'_>, input: &Value) -> Result<()> {
        self.check_knot_request(request, input)?;
        Ok(())
    }

    /// A scoped writer must hold a live Knot grant on this owner store's
    /// instance of the graph BEFORE an ID is consumed or an attempt sealed.
    /// Read-only; the effect transaction rechecks it before its COMMIT.
    fn check_scoped_grant(&self, store: &Store) -> Result<()> {
        store.check_writer_capability(
            crate::writer_scope::WriteOperation::Knot,
            &std::collections::BTreeSet::from([self.route.graph.clone()]),
        )
    }

    /// Execute one authenticated v2 attempt entirely in this owner's file.
    /// The original ID stays consumed after failure or an uncertain reply.
    /// Signed and cross-graph requests, and scoped writers without a Knot
    /// grant on this owner instance, refuse before admission.
    /// A single mailbox reservation spans admission, effect, deferred work
    /// and terminal recording; no legacy writer or receipt ledger is touched.
    pub fn knot_durable(
        &self,
        request: &WriteRequest<'_>,
        input: &Value,
        write_id: &str,
    ) -> Result<Value> {
        self.check_durable_request(request, input)?;
        let binding = Binding::for_request(write_id.to_owned(), input)?;
        let context = self.completion_context();
        let _admission = self.admit()?;
        let ticket = {
            let mut store = self.writer.owned()?;
            self.placement.verify()?;
            Self::check_identity(&self.route, &store)?;
            self.check_scoped_grant(&store)?;
            store.ownership_operation(|store| store.admit_owner_knot_attempt(binding, &context))?
        };
        let outcome = self.execute_knot(input, Some(&ticket));
        let mut store = self.writer.owned()?;
        self.placement.verify()?;
        Self::check_identity(&self.route, &store)?;
        store.ownership_operation(|store| {
            store.finish_knot_attempt(&ticket, outcome.as_ref().ok())
        })?;
        outcome
    }

    /// Exact owner-local evidence for the original authenticated request.
    /// Different body/principal/context is UNKNOWN or refused, never absence
    /// proof or permission to replay. Reads use the dedicated owner pool.
    pub fn knot_attempt_status(
        &self,
        request: &WriteRequest<'_>,
        input: &Value,
        write_id: &str,
    ) -> Result<Value> {
        self.check_durable_request(request, input)?;
        let binding = Binding::for_request(write_id.to_owned(), input)?;
        let context = self.completion_context();
        self.read(|store| store.owner_knot_attempt_status(&binding, &context))
    }

    /// Explicitly seal an uncommitted owner-bound attempt after restart.
    /// This retains the consumed ID and never authorizes a new execution.
    pub fn seal_knot_binding(
        &self,
        request: &WriteRequest<'_>,
        input: &Value,
        write_id: &str,
    ) -> Result<Value> {
        self.check_durable_request(request, input)?;
        let binding = Binding::for_request(write_id.to_owned(), input)?;
        let context = self.completion_context();
        let _admission = self.admit()?;
        let mut store = self.writer.owned()?;
        self.placement.verify()?;
        Self::check_identity(&self.route, &store)?;
        self.check_scoped_grant(&store)?;
        store.ownership_operation(|store| store.seal_owner_knot_binding(&binding, &context))
    }
}
