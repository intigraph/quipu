//! Default-deny, owner-enrolled ROOT admission. No transport is armed.
//!
//! This initial writer accepts conflict-free merges only. Resolver-signed
//! conflict decisions, named graphs and bounded temporal proposals refuse.
#[cfg(feature = "shacl")]
use std::collections::BTreeMap;

use crate::changeset_store::fact_statement;
#[cfg(feature = "shacl")]
use crate::changeset_store::{DiffSide, side_statements};
#[cfg(feature = "shacl")]
use crate::share_merge::{core, parse_graph};
use crate::{Result, Store};

#[cfg(feature = "shacl")]
use super::scope_digest;
use super::{BaseJournal, BaseRow, SignedChangeSet, digest, refuse};
mod grants;
mod receipts;
pub use grants::ClientWriteGrant;
pub use receipts::ChangesetReceipt;

const MAX_ROWS: usize = 10_000;

/// Reconstruct a complete, bounded ROOT base from the actual master history.
/// Future closes are restored to their open state at the requested transaction.
/// Bounded assertions are unsupported rather than flattened into open rows.
pub fn base_journal(store: &Store, tx: i64) -> Result<BaseJournal> {
    if tx < 1 || tx > store.latest_tx_id()? || store.get_transaction(tx)?.is_none() {
        return Err(refuse("base transaction unavailable"));
    }
    if store.fact_count_as_of(0, tx)? > MAX_ROWS {
        return Err(refuse("base row budget exceeded"));
    }
    let facts = store.facts_as_of_in_graph(
        &crate::store::AsOf {
            tx: Some(tx),
            valid_at: None,
        },
        0,
    )?;
    let mut rows = Vec::new();
    for fact in facts {
        if fact.valid_to.is_some() {
            let closed_after = store
                .conn
                .prepare(
                    "SELECT 1 FROM facts WHERE g=0 AND e=?1 AND a=?2 AND v=?3 AND tx=?4 \
                 AND valid_from=?5 AND retracted_tx>?6 LIMIT 1",
                )?
                .exists(rusqlite::params![
                    fact.entity,
                    fact.attribute,
                    fact.value.to_bytes(),
                    fact.tx,
                    fact.valid_from,
                    tx
                ])?;
            if !closed_after {
                return Err(refuse("bounded base intervals unsupported"));
            }
        }
        rows.push(BaseRow {
            triple: fact_statement(store, &fact, None)?.triple,
            valid_from: fact.valid_from,
            valid_to: None,
        });
    }
    let shapes = store
        .get_combined_shapes_as_of(&crate::store::AsOf {
            tx: Some(tx),
            valid_at: None,
        })?
        .unwrap_or_default();
    BaseJournal::new(
        store.store_id()?,
        store.log_epoch()?,
        tx,
        "ROOT".into(),
        digest(shapes.as_bytes()),
        rows,
    )
}

/// Admit one signed, conflict-free ROOT proposal under the actual writer lock.
/// The same operation and fingerprint returns its durable prior receipt.
/// No caller-provided clock, authority flag, base rows or validator verdict.
pub fn admit(store: &mut Store, proposal: &SignedChangeSet) -> Result<ChangesetReceipt> {
    if !store.conn.is_autocommit() {
        return Err(refuse(
            "master admission requires its own outermost transaction",
        ));
    }
    #[cfg(not(feature = "shacl"))]
    {
        let _ = (store, proposal);
        Err(refuse("master admission requires SHACL"))
    }
    #[cfg(feature = "shacl")]
    core::atomic(store, |store| admit_reserved(store, proposal))
}

#[cfg(feature = "shacl")]
fn admit_reserved(store: &mut Store, proposal: &SignedChangeSet) -> Result<ChangesetReceipt> {
    let e = &proposal.envelope;
    let cs = proposal.changeset()?;
    if e.graph != "ROOT"
        || e.audience != store.store_id()?
        || e.store_id != store.store_id()?
        || e.log_epoch != store.log_epoch()?
        || e.scope_digest != scope_digest(&["ROOT".into()], &[])
    {
        return Err(refuse("master lineage or ROOT scope mismatch"));
    }
    // A prior receipt is a read-back, not fresh authority to mutate. Still
    // verify exact signed bytes using the original accepted public key.
    if let Some(done) = receipts::lookup(store, &e.operation_id)? {
        if done.fingerprint != proposal.fingerprint()? {
            return Err(refuse("operation id reused with different signed content"));
        }
        proposal.verify(&done.public_key)?;
        return Ok(done);
    }
    let public_key = grants::authorize(store, proposal, &cs)?;
    proposal.verify(&public_key)?;
    let base = base_journal(store, e.base_tx)?;
    let shapes = store
        .get_combined_shapes()?
        .ok_or_else(|| refuse("master shapes unavailable"))?;
    if base.slice_hash()? != e.base_slice_hash
        || base.shapes_hash != e.shapes_hash
        || digest(shapes.as_bytes()) != e.shapes_hash
        || store.current_fact_count(0)? > MAX_ROWS
    {
        return Err(refuse("base, current shapes or row budget mismatch"));
    }
    let mut theirs = base.statements();
    for value in &cs.deletes {
        if !theirs.remove(value) {
            return Err(refuse("delete is not in frozen base"));
        }
    }
    for value in &cs.inserts {
        if !theirs.insert(value.clone()) {
            return Err(refuse("insert already in frozen base"));
        }
    }
    let render = |rows: &std::collections::BTreeSet<crate::changeset::Statement>| {
        rows.iter()
            .map(|s| format!("{} .\n", s.triple))
            .collect::<String>()
    };
    let b = parse_graph(&render(&base.statements()), "master base")?;
    let ours = side_statements(store, &DiffSide::Main, None)?;
    let o = parse_graph(&render(&ours), "master current")?;
    let t = parse_graph(&render(&theirs), "master proposal")?;
    let now = crate::time::now_iso();
    // Client claimed time is not authority or the source of valid-time writes.
    let plan = core::plan(
        store,
        &core::Inputs {
            base: &b,
            ours: &o,
            theirs: &t,
            shapes: &shapes,
            authored_at: &now,
        },
        &BTreeMap::new(),
        &now,
    )?;
    if plan.held != 0 {
        return Err(refuse(
            "merge conflict requires separately admitted decisions",
        ));
    }
    let (result, observations) = store.observe_validator(|store| {
        store.with_write_shapes(Some(&shapes), |store| {
            core::persist(
                store,
                &plan,
                &now,
                Some(&e.signer),
                &format!("client-changeset:{}", e.operation_id),
            )
        })
    });
    let accepted_tx = result?;
    if observations.is_empty()
        || observations
            .iter()
            .any(|v| v.graph() != 0 || v.gate_error().is_some() || v.capture_error().is_some())
    {
        return Err(refuse(
            "actual post-state validator observation missing or failed",
        ));
    }
    // Audit the actual full post-merge ROOT inside the same reserved writer,
    // in addition to its unchanged incremental write guards.
    if store.current_fact_count(0)? > MAX_ROWS {
        return Err(refuse("merged row budget exceeded"));
    }
    let after = side_statements(store, &DiffSide::Main, None)?;
    let feedback = crate::shacl::validate_shapes(&shapes, &render(&after))?;
    if !feedback.conforms || !feedback.results.is_empty() {
        return Err(refuse("actual merged ROOT does not conform"));
    }
    // Recheck at release time, never spend an expired/revoked grant based on
    // a pre-merge snapshot outside this transaction.
    if grants::authorize(store, proposal, &cs)? != public_key {
        return Err(refuse("current authority changed"));
    }
    let receipt = ChangesetReceipt {
        operation_id: e.operation_id.clone(),
        fingerprint: proposal.fingerprint()?,
        store_id: e.store_id.clone(),
        log_epoch: e.log_epoch.clone(),
        signer: e.signer.clone(),
        key_id: e.key_id.clone(),
        public_key,
        accepted_tx,
        accepted_at: now,
        verdict: "accepted".into(),
        post_state_hash: digest(render(&after).as_bytes()),
    };
    receipts::insert(store, &receipt)?;
    Ok(receipt)
}

#[cfg(all(test, feature = "shacl"))]
mod tests;
