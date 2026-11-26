//! One native ledger for legacy and owner-bound attempts. The original ID is
//! never re-keyed by generation: context drift must not turn a retry into a
//! fresh operation. Only admission may add the nullable private context column.
use super::*;

impl Store {
    /// Consume a fresh ID durably before dispatch. Capacity never evicts a
    /// prior ID. Native schema, context and attempt are installed atomically;
    /// ordinary RDF cannot read or amend them.
    pub fn admit_knot_attempt(&mut self, binding: Binding) -> Result<Ticket> {
        self.admit_knot_attempt_context(binding, None)
    }

    pub(crate) fn admit_owner_knot_attempt(
        &mut self,
        binding: Binding,
        context: &str,
    ) -> Result<Ticket> {
        if !hex64(context) {
            return Err(refused("invalid owner context digest"));
        }
        self.admit_knot_attempt_context(binding, Some(context))
    }

    fn admit_knot_attempt_context(
        &mut self,
        binding: Binding,
        context: Option<&str>,
    ) -> Result<Ticket> {
        binding.validate()?;
        self.ensure_ownership_writable()?;
        if !self.conn.is_autocommit() {
            return Err(refused("admission requires its own durable boundary"));
        }
        let ticket = Ticket {
            binding,
            store_id: self.store_id()?,
            epoch: self.log_epoch()?,
            nonce: super::super::open::new_urn_uuid("knot attempt")?,
            owner_context: context.map(str::to_owned),
        };
        self.conn.execute_batch("SAVEPOINT quipu_knot_admit")?;
        let outcome = (|| {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS knot_attempts (\
                 write_id TEXT PRIMARY KEY, owner TEXT NOT NULL, body TEXT NOT NULL, \
                 store_id TEXT NOT NULL, epoch TEXT NOT NULL, nonce TEXT NOT NULL, \
                 phase TEXT NOT NULL CHECK(phase IN ('admitted','committed','finished','stopped','sealed')), \
                 tx INTEGER, result TEXT, owner_context TEXT);",
            )?;
            if !self.has_knot_owner_context()? {
                self.conn
                    .execute_batch("ALTER TABLE knot_attempts ADD COLUMN owner_context TEXT")?;
            }
            // Reserve the writer before capacity and duplicate checks. The
            // context rides this same durable row, not a later metadata write.
            self.conn
                .execute("UPDATE knot_attempts SET phase=phase WHERE 0", [])?;
            let count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM (SELECT 1 FROM knot_attempts LIMIT ?1)",
                [MAX_ATTEMPTS],
                |row| row.get(0),
            )?;
            if count >= MAX_ATTEMPTS {
                return Err(refused("admission capacity reached"));
            }
            self.conn.execute(
                "INSERT INTO knot_attempts(write_id,owner,body,store_id,epoch,nonce,phase,owner_context) \
                 VALUES(?1,?2,?3,?4,?5,?6,'admitted',?7)",
                params![ticket.binding.write_id, ticket.binding.owner, ticket.binding.body_sha256,
                    ticket.store_id, ticket.epoch, ticket.nonce, ticket.owner_context],
            )?;
            Ok(())
        })();
        match outcome {
            Ok(()) => super::super::release::release(&self.conn, "quipu_knot_admit")?,
            Err(error) => {
                super::super::release::rollback_release(&self.conn, "quipu_knot_admit")?;
                return Err(error);
            }
        }
        Ok(ticket)
    }

    fn has_knot_owner_context(&self) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('knot_attempts') WHERE name='owner_context')",
            [], |row| row.get(0),
        )?)
    }

    pub(super) fn knot_attempt_context_matches(
        &self,
        id: &str,
        expected: Option<&str>,
    ) -> Result<bool> {
        // Reads of old ledgers stay read-only. A pre-context row belongs only
        // to legacy; it cannot stand in for an owner-bound receipt.
        if !self.has_knot_owner_context()? {
            return Ok(expected.is_none());
        }
        let context: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT owner_context FROM knot_attempts WHERE write_id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(context.is_some_and(|context| context.as_deref() == expected))
    }

    pub(crate) fn owner_knot_attempt_status(
        &self,
        binding: &Binding,
        context: &str,
    ) -> Result<Value> {
        self.knot_attempt_status_context(binding, Some(context))
    }

    pub(crate) fn seal_owner_knot_binding(
        &mut self,
        binding: &Binding,
        context: &str,
    ) -> Result<Value> {
        self.seal_knot_binding_context(binding, Some(context))
    }
}

#[cfg(test)]
#[path = "knot_completion_owner_tests.rs"]
mod tests;
