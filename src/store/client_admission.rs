//! Current-authority admission of a signed client overlay, never follower replay.
//!
//! This first, unwired library slice accepts one complete graph with open
//! intervals. Missing context, explicit proposed intervals, stale shapes and
//! other unsupported scopes refuse; none are treated as successful validation.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::Store;
use crate::client_changeset::{SignedChangeSet, digest, refuse};
use crate::governance::verifier_registry::{self, Scope, Witness};
use crate::share_merge::{DecisionRecord, Graph};
use crate::{Error, Result};

mod base;
#[cfg(test)]
mod tests;

/// Immutable admission result, retained in the same transaction as the facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangesetReceipt {
    /// Idempotency key bound by the client signature.
    pub operation_id: String,
    /// Digest of every signed binding.
    pub fingerprint: String,
    /// Accepting store lineage.
    pub store_id: String,
    /// Accepting source epoch.
    pub log_epoch: String,
    /// Exact admitted graph scope.
    pub scope_digest: String,
    /// Fact-log transaction holding the accepted change.
    pub accepted_tx: i64,
    /// Verifier whose current authority admitted the proposal.
    pub signer: String,
    /// Governed key registration used at admission.
    pub key_id: String,
    /// Master-assigned admission time.
    pub accepted_at: String,
    /// Explicit accepted verdict.
    pub verdict: String,
}

/// Conflicts never commit a partial proposal. A replay returns the identical
/// receipt and no additional transaction, even if a prior response was lost.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Admission {
    /// Accepted once or read back after an identical retry.
    Accepted {
        receipt: Box<ChangesetReceipt>,
        replayed: bool,
    },
    /// No accepted write; the client retains its unresolved overlay.
    Conflicts { records: Vec<DecisionRecord> },
}

/// Exact capability a governed registration must grant for each touched slot.
/// Registration, key and capability must be facts of the SAME trust-root
/// governed registration. This function neither registers nor endorses a key.
pub fn write_capability(graph: &str, predicate: &str) -> String {
    serde_json::json!([crate::client_changeset::DOMAIN, graph, predicate]).to_string()
}

fn authorize(
    store: &Store,
    proposal: &SignedChangeSet,
    cs: &crate::changeset::ChangeSet,
) -> Result<()> {
    let e = &proposal.envelope;
    let witness = Witness::now();
    let keys =
        verifier_registry::keyed_registrations(store, &e.signer, None, &witness, Scope::Governed)?;
    let key = keys
        .iter()
        .find(|key| key.registration == e.key_id)
        .ok_or_else(|| refuse("signer is not currently registered"))?;
    proposal.verify(&key.key)?;
    let predicates = cs
        .deletes
        .iter()
        .chain(&cs.inserts)
        .map(|s| s.to_quad().map(|q| q.predicate.as_str().to_owned()))
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    if predicates.is_empty() {
        return Err(refuse("empty proposal"));
    }
    for predicate in predicates {
        let capability = write_capability(&e.graph, &predicate);
        let allowed = verifier_registry::keyed_registrations(
            store,
            &e.signer,
            Some(&capability),
            &witness,
            Scope::Governed,
        )?;
        if !allowed.iter().any(|candidate| candidate == key) {
            return Err(refuse("signer lacks current graph/predicate authority"));
        }
    }
    Ok(())
}

fn graph(rows: &[crate::client_changeset::BaseRow]) -> Result<Graph> {
    crate::share_merge::parse_graph(
        &rows
            .iter()
            .map(|r| format!("{} .\n", r.triple))
            .collect::<String>(),
        "client journal",
    )
}

fn render(graph: &Graph) -> String {
    let mut rows: Vec<_> = graph.iter().map(|t| format!("{t} .\n")).collect();
    rows.sort();
    rows.concat()
}

fn validate_candidate(shapes: &str, candidate: &Graph) -> Result<()> {
    #[cfg(feature = "shacl")]
    {
        crate::shacl::cached_validator(shapes)?.validate_or_reject(render(candidate).as_bytes())
    }
    #[cfg(not(feature = "shacl"))]
    {
        let _ = (shapes, candidate);
        Err(refuse("full SHACL is unavailable"))
    }
}

impl Store {
    /// Read a retained receipt. A transport exposing this privileged library
    /// method must authorize the caller; there is intentionally no new route.
    pub fn client_changeset_receipt(&self, operation_id: &str) -> Result<Option<ChangesetReceipt>> {
        let row: Option<String> = self
            .conn
            .query_row(
                "SELECT receipt FROM client_changeset_receipts WHERE operation_id=?1",
                [operation_id],
                |r| r.get(0),
            )
            .optional()?;
        row.map(|s| serde_json::from_str(&s).map_err(|e| Error::Serialization(e.to_string())))
            .transpose()
    }

    /// Admit one signed proposal through a current registry, three-way merge,
    /// normal graph gates and full current-shapes validation, atomically.
    /// A refused proposal leaves its client overlay/base journal untouched.
    pub fn admit_client_changeset(&mut self, proposal: &SignedChangeSet) -> Result<Admission> {
        if !self.conn.is_autocommit() {
            return Err(refuse(
                "admission requires an independent writer transaction",
            ));
        }
        self.ensure_ownership_writable()?;
        self.conn.execute_batch("SAVEPOINT quipu_client_admit")?;
        self.set_write_in_progress(true);
        let timestamp = crate::time::now_iso();
        let source = format!("client-changeset:{}", proposal.envelope.operation_id);
        let mut audit = None;
        let result = (|| {
            // Writer reservation precedes every registration/base/receipt read.
            self.conn
                .execute("UPDATE transactions SET id=id WHERE 0", [])?;
            let cs = proposal.changeset()?;
            let e = &proposal.envelope;
            if e.store_id != self.store_id()? || e.audience != self.store_id()? {
                return Err(refuse("foreign-store or audience"));
            }
            if e.log_epoch != self.log_epoch()? {
                return Err(refuse("foreign-epoch"));
            }
            if e.scope_digest
                != crate::asof_share::scope_digest(std::slice::from_ref(&e.graph), &[])
            {
                return Err(refuse("scope-mismatch"));
            }
            authorize(self, proposal, &cs)?;
            let fingerprint = proposal.fingerprint()?;
            if let Some(receipt) = self.client_changeset_receipt(&e.operation_id)? {
                if receipt.fingerprint != fingerprint {
                    return Err(refuse("operation_id reused with different content"));
                }
                return Ok((
                    Admission::Accepted {
                        receipt: Box::new(receipt),
                        replayed: true,
                    },
                    None,
                ));
            }
            let journal = base::journal(self, &e.graph, e.base_tx)?;
            if journal.slice_hash()? != e.base_slice_hash {
                return Err(refuse("base-mismatch"));
            }
            let shapes = self
                .get_combined_shapes()?
                .ok_or_else(|| refuse("UNKNOWN current shapes"))?;
            if digest(shapes.as_bytes()) != e.shapes_hash || journal.shapes_hash != e.shapes_hash {
                return Err(refuse("stale-shapes"));
            }
            let id = base::graph_id(self, &e.graph)?;
            let base = graph(&journal.rows().iter().cloned().collect::<Vec<_>>())?;
            let ours = graph(&base::rows(self, id, self.latest_tx_id()?)?)?;
            let mut theirs = base.clone();
            for (side, delete) in [(&cs.deletes, true), (&cs.inserts, false)] {
                for statement in side {
                    if crate::changeset_store::canonical(self, statement)? != *statement {
                        return Err(refuse("noncanonical store literal"));
                    }
                    let triple = oxrdf::Triple::from(statement.to_quad()?);
                    if delete {
                        if !theirs.remove(&triple) {
                            return Err(refuse("delete absent from frozen base"));
                        }
                    } else {
                        theirs.insert(triple);
                    }
                }
            }
            let (merged, records) =
                crate::share_merge::merge_graphs(&base, &ours, &theirs, &shapes)?;
            if !records.is_empty() {
                return Ok((Admission::Conflicts { records }, None));
            }
            self.assert_write_target_allowed(id, Some(&source))?;
            audit = Some(id);
            self.enforce_graph_authority(id)?;
            let additions = merged.difference(&ours).cloned().collect();
            let removals = ours.difference(&merged).cloned().collect();
            let mut datums = crate::rdf::parse_rdf(
                self,
                render(&additions).as_bytes(),
                oxrdfio::RdfFormat::NTriples,
                None,
                &timestamp,
            )?;
            let mut retracts = crate::rdf::parse_rdf(
                self,
                render(&removals).as_bytes(),
                oxrdfio::RdfFormat::NTriples,
                None,
                &timestamp,
            )?;
            for datum in &mut retracts {
                datum.op = crate::Op::Retract;
            }
            datums.extend(retracts);
            let staged = self.stage_and_guard(
                &datums,
                &timestamp,
                Some(&e.signer),
                Some(&source),
                id,
                None,
            )?;
            let tx = self.latest_tx_id()?;
            self.enforce_shacl_poststate(&datums, id, tx, true)?;
            // Validate the ACTUAL complete staged graph, including any normal
            // OWL inferences; neither payload-only nor incremental SHACL suffices.
            let candidate = graph(&base::rows(self, id, tx)?)?;
            validate_candidate(&shapes, &candidate)?;
            crate::promotion_control::claim_commit(Some(&source))?;
            let receipt = ChangesetReceipt {
                operation_id: e.operation_id.clone(),
                fingerprint: fingerprint.clone(),
                store_id: e.store_id.clone(),
                log_epoch: e.log_epoch.clone(),
                scope_digest: e.scope_digest.clone(),
                accepted_tx: tx,
                signer: e.signer.clone(),
                key_id: e.key_id.clone(),
                accepted_at: timestamp.clone(),
                verdict: "accepted".into(),
            };
            self.conn.execute("INSERT INTO client_changeset_receipts(operation_id,fingerprint,receipt,tx) VALUES(?1,?2,?3,?4)",
                params![e.operation_id, fingerprint, serde_json::to_string(&receipt)
                    .map_err(|e| Error::Serialization(e.to_string()))?, tx])?;
            Ok((
                Admission::Accepted {
                    receipt: Box::new(receipt),
                    replayed: false,
                },
                Some((datums, staged, id)),
            ))
        })();
        match result {
            Ok((outcome, write)) => {
                if let Err(error) = super::release::release(&self.conn, "quipu_client_admit") {
                    let _ = super::release::rollback_release(&self.conn, "quipu_client_admit");
                    self.set_write_in_progress(false);
                    self.term_cache.borrow_mut().clear_persistent();
                    self.invalidate_read_model();
                    return Err(error.into());
                }
                self.set_write_in_progress(false);
                if let Some((datums, staged, id)) = write {
                    self.invalidate_read_model();
                    self.after_commit_hooks(&datums, staged, &timestamp, Some(&source), id)?;
                    self.invalidate_policy_registry_if_governance(&datums)?;
                    self.flush_pending_verdicts(&timestamp, Some(&proposal.envelope.signer));
                }
                Ok(outcome)
            }
            Err(error) => {
                let rollback = super::release::rollback_release(&self.conn, "quipu_client_admit");
                self.set_write_in_progress(false);
                self.term_cache.borrow_mut().clear_persistent();
                self.invalidate_read_model();
                let refusal = self.pending_refusal.take();
                self.clear_pending_write_events();
                self.pending_requests.clear();
                if let Some(id) = audit {
                    self.flush_pending_verdicts(&timestamp, Some(&proposal.envelope.signer));
                    if let Some(refusal) = refusal {
                        self.record_refusal(
                            &refusal,
                            &self.graph_iri_of(id),
                            Some(&proposal.envelope.signer),
                            Some(&source),
                            &timestamp,
                        );
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
