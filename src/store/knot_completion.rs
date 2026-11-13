//! Durable receiver admission and commit evidence. No replay or expiry API.
//! Transport must bind `owner` to authenticated stable credentials, not headers
//! declaring provenance. Native admission consumes an ID even on later failure.
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::Store;
use crate::{Error, Result};

const MAX_ATTEMPTS: i64 = 65536;

/// Exact authenticated request binding; never contains a bearer secret.
#[derive(Clone, Debug)]
pub struct Binding {
    /// Fresh lowercase SHA256-sized write ID.
    pub write_id: String,
    /// Canonical principal, stable credential ID and authentication class.
    pub owner: String,
    /// SHA256 of the original canonical JSON object.
    pub body_sha256: String,
}

/// Receiver-issued capability for ONE attempt, not authority to replay it.
#[derive(Clone, Debug)]
pub struct Ticket {
    binding: Binding,
    store_id: String,
    epoch: String,
    nonce: String,
}

fn refused(why: &str) -> Error {
    Error::InvalidValue(format!("knot completion: {why}"))
}

fn hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

impl Binding {
    /// Bind to the authenticated thread scope and original JSON object.
    pub fn for_request(write_id: String, request: &Value) -> Result<Self> {
        if !request.is_object() {
            return Err(refused("request must be an object"));
        }
        let bytes = serde_json::to_vec(request).map_err(|e| refused(&e.to_string()))?;
        let binding = Self {
            write_id,
            owner: current_owner()?,
            body_sha256: format!("{:x}", Sha256::digest(bytes)),
        };
        binding.validate()?;
        Ok(binding)
    }
    fn validate(&self) -> Result<()> {
        if !hex64(&self.write_id)
            || !hex64(&self.body_sha256)
            || self.owner != current_owner()?
            || self.owner.is_empty()
            || self.owner == "null"
            || self.owner.len() > 4096
        {
            return Err(refused("invalid authenticated request binding"));
        }
        Ok(())
    }
}

impl Ticket {
    pub(crate) fn check_request(&self, request: &Value) -> Result<()> {
        let current = Binding::for_request(self.binding.write_id.clone(), request)?;
        if current.owner != self.binding.owner || current.body_sha256 != self.binding.body_sha256 {
            return Err(refused("original request binding changed"));
        }
        Ok(())
    }
}

fn current_owner() -> Result<String> {
    let i = crate::transaction_auth::current_identity()
        .ok_or_else(|| refused("authenticated owner required"))?;
    if i.principal.is_empty()
        || i.credential_id.as_ref().is_none_or(String::is_empty)
        || i.auth_class.is_empty()
    {
        return Err(refused("stable authenticated credential required"));
    }
    serde_json::to_string(&(i.principal, i.credential_id, i.auth_class))
        .map_err(|e| refused(&e.to_string()))
}

impl Store {
    /// The ordinary graph write with one explicit receiver ticket. Its witness
    /// rides the existing transaction; audits and post-commit observers never
    /// inherit the ticket.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn transact_knot_to_graph(
        &mut self,
        datums: &[super::Datum],
        timestamp: &str,
        actor: Option<&str>,
        source: Option<&str>,
        graph: i64,
        retract_source: Option<&str>,
        ticket: Option<&Ticket>,
    ) -> Result<i64> {
        self.transact_with_owl_receipt(
            datums,
            timestamp,
            actor,
            source,
            graph,
            retract_source,
            &[],
            None,
            None,
            None,
            None,
            None,
            ticket,
        )
    }

    /// Consume a fresh ID durably, before dispatch. Capacity never evicts a
    /// prior ID. Lazy protected schema is native-only; ordinary RDF cannot
    /// read or amend it. Admission inside another transaction is refused.
    pub fn admit_knot_attempt(&mut self, binding: Binding) -> Result<Ticket> {
        binding.validate()?;
        self.ensure_ownership_writable()?;
        if !self.conn.is_autocommit() {
            return Err(refused("admission requires its own durable boundary"));
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS knot_attempts (\
             write_id TEXT PRIMARY KEY, owner TEXT NOT NULL, body TEXT NOT NULL, \
             store_id TEXT NOT NULL, epoch TEXT NOT NULL, nonce TEXT NOT NULL, \
             phase TEXT NOT NULL CHECK(phase IN ('admitted','committed','finished','stopped','sealed')), \
             tx INTEGER, result TEXT);",
        )?;
        let ticket = Ticket {
            binding,
            store_id: self.store_id()?,
            epoch: self.log_epoch()?,
            nonce: super::open::new_urn_uuid("knot attempt")?,
        };
        self.conn.execute_batch("SAVEPOINT quipu_knot_admit")?;
        let outcome = (|| {
            // Reserve the writer before the capacity and duplicate checks.
            self.conn
                .execute("UPDATE knot_attempts SET phase=phase WHERE 0", [])?;
            let count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM (SELECT 1 FROM knot_attempts LIMIT ?1)",
                [MAX_ATTEMPTS],
                |r| r.get(0),
            )?;
            if count >= MAX_ATTEMPTS {
                return Err(refused("admission capacity reached"));
            }
            self.conn.execute(
                "INSERT INTO knot_attempts(write_id,owner,body,store_id,epoch,nonce,phase) \
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
            Ok(())
        })();
        match outcome {
            Ok(()) => super::release::release(&self.conn, "quipu_knot_admit")?,
            Err(e) => {
                super::release::rollback_release(&self.conn, "quipu_knot_admit")?;
                return Err(e);
            }
        }
        Ok(ticket)
    }

    fn check_knot_ticket(&self, ticket: &Ticket) -> Result<()> {
        ticket.binding.validate()?;
        if ticket.store_id != self.store_id()? || ticket.epoch != self.log_epoch()? {
            return Err(refused("store identity or epoch changed"));
        }
        Ok(())
    }

    /// Called ONLY inside the intended effect's existing native savepoint,
    /// after its ordinary gates and before its real RELEASE. A sealed ticket
    /// makes that whole transaction roll back. No outer transaction is added.
    pub(in crate::store) fn commit_knot_attempt(&self, ticket: &Ticket, tx: i64) -> Result<()> {
        self.check_knot_ticket(ticket)?;
        if self.conn.is_autocommit() {
            return Err(refused("commit witness needs the effect transaction"));
        }
        let changed = self.conn.execute(
            "UPDATE knot_attempts SET phase='committed',tx=?7 \
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

    /// Worker-only terminal update, after synchronous and deferred work ended.
    /// Errors retain any atomic commit witness. A result is bounded and
    /// restricted to the existing completion fields, never a request body.
    pub fn finish_knot_attempt(&mut self, ticket: &Ticket, result: Option<&Value>) -> Result<()> {
        self.ensure_ownership_writable()?;
        self.check_knot_ticket(ticket)?;
        if !self.conn.is_autocommit() {
            return Err(refused("completion before native commit ended"));
        }
        let compact = result.map(|r| {
            let mut v = serde_json::Map::new();
            for k in ["tx_id", "count", "conforms", "replaced"] {
                if let Some(x) = r.get(k) {
                    v.insert(k.into(), x.clone());
                }
            }
            Value::Object(v)
        });
        let encoded = compact.map(|v| v.to_string());
        if encoded.as_ref().is_some_and(|s| s.len() > 4096) {
            return Err(refused("receipt too large"));
        }
        if let Some(result) = result {
            let committed: Option<i64> = self.conn.query_row(
                "SELECT tx FROM knot_attempts WHERE write_id=?1 AND nonce=?2",
                params![ticket.binding.write_id, ticket.nonce],
                |r| r.get(0),
            )?;
            if committed.is_some_and(|tx| result.get("tx_id").and_then(Value::as_i64) != Some(tx))
                || (committed.is_none() && result.get("conforms") != Some(&Value::Bool(false)))
            {
                return Err(refused("successful result lacks its atomic commit witness"));
            }
        }
        let changed = self.conn.execute(
            "UPDATE knot_attempts SET phase=?7,result=?8 \
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

    /// Explicit native sealing, not a status read. Serialized with effect
    /// commits by SQLite; a committed attempt cannot be sealed. A late worker
    /// must check its ticket inside its effect transaction and roll back.
    pub fn seal_knot_attempt(&mut self, ticket: &Ticket) -> Result<()> {
        self.ensure_ownership_writable()?;
        self.check_knot_ticket(ticket)?;
        if !self.conn.is_autocommit() {
            return Err(refused("seal requires its own durable boundary"));
        }
        let changed = self.conn.execute(
            "UPDATE knot_attempts SET phase='sealed' WHERE write_id=?1 AND owner=?2 AND body=?3 \
             AND store_id=?4 AND epoch=?5 AND nonce=?6 AND phase='admitted'",
            params![
                ticket.binding.write_id,
                ticket.binding.owner,
                ticket.binding.body_sha256,
                ticket.store_id,
                ticket.epoch,
                ticket.nonce
            ],
        )?;
        if changed != 1 {
            return Err(refused("attempt already committed or terminal"));
        }
        Ok(())
    }

    /// Explicit owner-bound sealing after a receiver restart. The immutable
    /// write ID and full binding select the retained attempt; SQLite orders
    /// this seal against its commit-time ticket check. This is not a status
    /// side effect and does not grant permission to replay the request.
    pub fn seal_knot_binding(&mut self, binding: &Binding) -> Result<Value> {
        binding.validate()?;
        self.ensure_ownership_writable()?;
        if !self.conn.is_autocommit() {
            return Err(refused("seal requires its own durable boundary"));
        }
        let changed = self.conn.execute(
            "UPDATE knot_attempts SET phase='sealed' WHERE write_id=?1 AND owner=?2 AND body=?3 \
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
        self.knot_attempt_status(binding)
    }

    /// Whether a v2 write ID was ever consumed, regardless of its owner.
    /// Used only to refuse a legacy downgrade, never to reveal a receipt.
    pub fn knot_write_id_consumed(&self, id: &str) -> Result<bool> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='knot_attempts')", [], |r| r.get(0))?;
        if !exists {
            return Ok(false);
        }
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM knot_attempts WHERE write_id=?1)",
            [id],
            |r| r.get(0),
        )?)
    }

    /// Read exact v2 evidence. Missing/foreign bindings are UNKNOWN, never
    /// negative execution proof. Does not create schema, admit, seal or replay.
    pub fn knot_attempt_status(&self, binding: &Binding) -> Result<Value> {
        binding.validate()?;
        let unknown = || json!({"contract":"quipu.knot-completion/v2","write_id":binding.write_id,"state":"unknown"});
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='knot_attempts')", [], |r| r.get(0))?;
        if !exists {
            return Ok(unknown());
        }
        let row: Option<(String, Option<i64>, Option<String>)> = self.conn.query_row(
            "SELECT phase,tx,result FROM knot_attempts WHERE write_id=?1 AND owner=?2 AND body=?3 \
             AND store_id=?4 AND epoch=?5",
            params![binding.write_id, binding.owner, binding.body_sha256, self.store_id()?, self.log_epoch()?],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let Some((phase, tx, result)) = row else {
            return Ok(unknown());
        };
        let mut response = json!({"contract":"quipu.knot-completion/v2","write_id":binding.write_id,
            "state":phase,"store_id":self.store_id()?,"log_epoch":self.log_epoch()?});
        if let Some(tx) = tx {
            response["commit"] = json!({"tx_id":tx});
        }
        if let Some(result) = result {
            response["receipt"] =
                serde_json::from_str(&result).map_err(|_| refused("corrupt receipt"))?;
        }
        Ok(response)
    }
}

#[cfg(test)]
#[path = "knot_completion_tests.rs"]
mod tests;
