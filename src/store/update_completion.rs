//! Durable write IDs for routed `/update` (aegis-n0wc.5 H3).
//!
//! The SAME contract as `/knot` v2 (`x-quipu-completion-contract` plus one
//! `x-quipu-write-id`), recorded in its own `update_attempts` table in the
//! owner file, so existing `knot_attempts` rows are never touched. A fresh ID
//! is admitted durably BEFORE the effect; the commit witness rides the
//! update's own multi-graph savepoint, so data, provenance and witness commit
//! or roll back together; the complete receipt is recorded after the effect.
//!
//! A retry with the same ID, principal and bytes returns the recorded outcome
//! and never re-runs. The same ID with different bytes or a different
//! principal is refused with the same message and no receipt. An attempt
//! left `admitted` (a crash before commit) is UNKNOWN custody: it can be
//! sealed, after which a NEW ID may retry the intent; it never runs again.
use std::cell::RefCell;

use super::{Binding, MAX_ATTEMPTS, refused};
use crate::Result;
use crate::store::Store;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

/// The status `contract` field (the request contract is `/knot` v2's).
pub const STATUS_CONTRACT: &str = "quipu.update-completion/v1";
/// Largest recorded receipt.
const MAX_RECEIPT: usize = 65536;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS update_attempts (\
     write_id TEXT PRIMARY KEY, owner TEXT NOT NULL, body TEXT NOT NULL, \
     store_id TEXT NOT NULL, epoch TEXT NOT NULL, nonce TEXT NOT NULL, \
     phase TEXT NOT NULL CHECK(phase IN ('admitted','committed','finished','stopped','sealed')), \
     tx INTEGER, receipt TEXT);";

/// Receiver-issued capability for ONE routed update attempt.
#[derive(Clone, Debug)]
pub struct UpdateTicket {
    binding: Binding,
    store_id: String,
    epoch: String,
    nonce: String,
}

/// What admitting a write ID found.
#[derive(Debug)]
pub enum UpdateAdmission {
    /// A fresh ID: run the update with this ticket.
    Fresh(UpdateTicket),
    /// The same ID, principal and bytes were already admitted: its recorded
    /// status (see [`Store::update_attempt_status`]). Never run it again.
    Retry(Value),
}

thread_local! {
    static PENDING: RefCell<Option<UpdateTicket>> = const { RefCell::new(None) };
}

/// Run `work` with `ticket` pending: the first multi-graph commit inside it
/// records the commit witness in its own savepoint. Returns whether a
/// witness was recorded. The slot is cleared on every exit.
pub fn with_update_ticket<T>(ticket: &UpdateTicket, work: impl FnOnce() -> T) -> (T, bool) {
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            PENDING.with(|slot| slot.borrow_mut().take());
        }
    }
    PENDING.with(|slot| *slot.borrow_mut() = Some(ticket.clone()));
    let clear = Clear;
    let out = work();
    let witnessed = PENDING.with(|slot| slot.borrow().is_none());
    drop(clear);
    (out, witnessed)
}

/// The pending ticket, consumed by the commit that witnesses it.
pub(in crate::store) fn take_pending() -> Option<UpdateTicket> {
    PENDING.with(|slot| slot.borrow_mut().take())
}

impl Store {
    fn update_ledger_exists(&self) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='update_attempts')",
            [],
            |r| r.get(0),
        )?)
    }

    fn check_update_ticket(&self, ticket: &UpdateTicket) -> Result<()> {
        ticket.binding.validate()?;
        if ticket.store_id != self.store_id()? || ticket.epoch != self.log_epoch()? {
            return Err(refused("store identity or epoch changed"));
        }
        Ok(())
    }

    /// Consume a write ID durably before the effect, or find its earlier
    /// attempt. The same ID bound to another principal or other bytes is
    /// refused with ONE message, revealing nothing about the original.
    pub fn admit_update_attempt(&mut self, binding: &Binding) -> Result<UpdateAdmission> {
        binding.validate()?;
        self.ensure_ownership_writable()?;
        if !self.conn.is_autocommit() {
            return Err(refused("admission requires its own durable boundary"));
        }
        self.conn.execute_batch("SAVEPOINT quipu_update_admit")?;
        let outcome = (|| {
            self.conn.execute_batch(SCHEMA)?;
            // Reserve the writer before the duplicate and capacity checks.
            self.conn
                .execute("UPDATE update_attempts SET phase=phase WHERE 0", [])?;
            let prior: Option<(String, String)> = self
                .conn
                .query_row(
                    "SELECT owner, body FROM update_attempts WHERE write_id=?1",
                    [&binding.write_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((owner, body)) = prior {
                if owner != binding.owner || body != binding.body_sha256 {
                    return Err(refused("write ID already used"));
                }
                return Ok(None);
            }
            let count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM (SELECT 1 FROM update_attempts LIMIT ?1)",
                [MAX_ATTEMPTS],
                |row| row.get(0),
            )?;
            if count >= MAX_ATTEMPTS {
                return Err(refused("admission capacity reached"));
            }
            let ticket = UpdateTicket {
                binding: binding.clone(),
                store_id: self.store_id()?,
                epoch: self.log_epoch()?,
                nonce: super::super::open::new_urn_uuid("update attempt")?,
            };
            self.conn.execute(
                "INSERT INTO update_attempts(write_id,owner,body,store_id,epoch,nonce,phase) \
                 VALUES(?1,?2,?3,?4,?5,?6,'admitted')",
                params![
                    ticket.binding.write_id,
                    ticket.binding.owner,
                    ticket.binding.body_sha256,
                    ticket.store_id,
                    ticket.epoch,
                    ticket.nonce
                ],
            )?;
            Ok(Some(ticket))
        })();
        match outcome {
            Ok(fresh) => {
                super::super::release::release(&self.conn, "quipu_update_admit")?;
                Ok(match fresh {
                    Some(ticket) => UpdateAdmission::Fresh(ticket),
                    None => UpdateAdmission::Retry(self.update_attempt_status(binding)?),
                })
            }
            Err(error) => {
                super::super::release::rollback_release(&self.conn, "quipu_update_admit")?;
                Err(error)
            }
        }
    }

    /// The commit witness: called ONLY inside the update's own savepoint,
    /// before its RELEASE. A sealed or foreign attempt rolls the write back.
    pub(in crate::store) fn commit_update_attempt(
        &self,
        ticket: &UpdateTicket,
        tx: i64,
    ) -> Result<()> {
        self.check_update_ticket(ticket)?;
        if self.conn.is_autocommit() {
            return Err(refused("commit witness needs the effect transaction"));
        }
        let changed = self.conn.execute(
            "UPDATE update_attempts SET phase='committed',tx=?7 \
             WHERE write_id=?1 AND owner=?2 AND body=?3 AND store_id=?4 AND epoch=?5 \
             AND nonce=?6 AND phase='admitted'",
            params![
                ticket.binding.write_id,
                ticket.binding.owner,
                ticket.binding.body_sha256,
                ticket.store_id,
                ticket.epoch,
                ticket.nonce,
                tx
            ],
        )?;
        if changed != 1 {
            return Err(refused("attempt no longer admits this worker"));
        }
        Ok(())
    }

    /// Record the complete receipt after the effect (or `None` when it
    /// failed). A receipt claiming a commit must match the atomic witness,
    /// and a witnessed commit cannot be recorded as stopped.
    pub fn finish_update_attempt(
        &mut self,
        ticket: &UpdateTicket,
        receipt: Option<&Value>,
    ) -> Result<()> {
        self.ensure_ownership_writable()?;
        self.check_update_ticket(ticket)?;
        if !self.conn.is_autocommit() {
            return Err(refused("completion before native commit ended"));
        }
        let encoded = receipt.map(Value::to_string);
        if encoded.as_ref().is_some_and(|s| s.len() > MAX_RECEIPT) {
            return Err(refused("receipt too large"));
        }
        let committed: Option<i64> = self.conn.query_row(
            "SELECT tx FROM update_attempts WHERE write_id=?1 AND nonce=?2",
            params![ticket.binding.write_id, ticket.nonce],
            |r| r.get(0),
        )?;
        let claimed = receipt.and_then(|r| r["owner"]["local_tx"].as_i64());
        match (receipt, committed) {
            (Some(_), Some(tx)) if claimed != Some(tx) => {
                return Err(refused("receipt does not match its commit witness"));
            }
            (Some(_), None) if claimed.is_some() => {
                return Err(refused("successful result lacks its atomic commit witness"));
            }
            (None, Some(_)) => {
                return Err(refused("a witnessed commit cannot be recorded as stopped"));
            }
            _ => {}
        }
        let changed = self.conn.execute(
            "UPDATE update_attempts SET phase=?7,receipt=?8 \
             WHERE write_id=?1 AND owner=?2 AND body=?3 AND store_id=?4 AND epoch=?5 \
             AND nonce=?6 AND phase IN ('admitted','committed')",
            params![
                ticket.binding.write_id,
                ticket.binding.owner,
                ticket.binding.body_sha256,
                ticket.store_id,
                ticket.epoch,
                ticket.nonce,
                if encoded.is_some() {
                    "finished"
                } else {
                    "stopped"
                },
                encoded
            ],
        )?;
        if changed != 1 {
            return Err(refused("terminal or fenced attempt cannot be replaced"));
        }
        Ok(())
    }

    /// Seal an attempt left `admitted` (UNKNOWN custody, e.g. after a crash),
    /// so no late worker can commit it; the caller may then retry the intent
    /// with a NEW write ID. A committed attempt cannot be sealed.
    pub fn seal_update_binding(&mut self, binding: &Binding) -> Result<Value> {
        binding.validate()?;
        self.ensure_ownership_writable()?;
        if !self.conn.is_autocommit() {
            return Err(refused("seal requires its own durable boundary"));
        }
        if !self.update_ledger_exists()? {
            return Err(refused("attempt absent, committed or terminal"));
        }
        let changed = self.conn.execute(
            "UPDATE update_attempts SET phase='sealed' WHERE write_id=?1 AND owner=?2 AND body=?3 \
             AND store_id=?4 AND epoch=?5 AND phase='admitted'",
            params![
                binding.write_id,
                binding.owner,
                binding.body_sha256,
                self.store_id()?,
                self.log_epoch()?
            ],
        )?;
        if changed != 1 {
            return Err(refused("attempt absent, committed or terminal"));
        }
        self.update_attempt_status(binding)
    }

    /// Exact evidence for one binding. A missing or foreign binding is
    /// `unknown`, never negative execution proof. Creates no schema.
    pub fn update_attempt_status(&self, binding: &Binding) -> Result<Value> {
        binding.validate()?;
        let unknown = || json!({"contract": STATUS_CONTRACT, "write_id": binding.write_id, "state": "unknown"});
        if !self.update_ledger_exists()? {
            return Ok(unknown());
        }
        let row: Option<(String, Option<i64>, Option<String>)> = self
            .conn
            .query_row(
                "SELECT phase,tx,receipt FROM update_attempts WHERE write_id=?1 AND owner=?2 \
                 AND body=?3 AND store_id=?4 AND epoch=?5",
                params![
                    binding.write_id,
                    binding.owner,
                    binding.body_sha256,
                    self.store_id()?,
                    self.log_epoch()?
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((phase, tx, receipt)) = row else {
            return Ok(unknown());
        };
        let mut response = json!({"contract": STATUS_CONTRACT, "write_id": binding.write_id,
            "state": phase, "store_id": self.store_id()?, "log_epoch": self.log_epoch()?});
        if let Some(tx) = tx {
            response["commit"] = json!({"tx_id": tx});
        }
        if let Some(receipt) = receipt {
            response["receipt"] =
                serde_json::from_str(&receipt).map_err(|_| refused("corrupt receipt"))?;
        }
        Ok(response)
    }
}
