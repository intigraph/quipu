//! SQLite backstop for normal adapters that bypass the Store staging API.
use super::{Datum, Store};
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};

fn ref_blob(id: &str) -> String {
    // Concatenation copies bytes; CAST retains BLOB identity, including NULs.
    // Avoid connection-local SQL functions: offline import opens its own handle.
    let bytes: String = (0_u16..=255).map(|byte| format!("{byte:02X}")).collect();
    let parts: Vec<_> = (0..8)
        .map(|byte| format!("substr(X'{bytes}', (({id} >> {}) & 255) + 1, 1)", byte * 8))
        .collect();
    format!(
        "CAST(X'{:02X}' || {} AS BLOB)",
        crate::types::TAG_REF,
        parts.join(" || ")
    )
}

fn protected(record: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM facts AS prior JOIN fluent_type_codes AS kind ON prior.v=kind.value WHERE prior.e={record}.e AND prior.a=(SELECT id FROM terms WHERE iri='{}'))",
        crate::namespace::RDF_TYPE
    )
}

impl Store {
    pub(super) fn migrate_fluent_persistence(conn: &Connection) -> Result<()> {
        crate::fluent::pending_config::storage::migrate(conn)?;
        let qp = crate::namespace::QUIPU_FLUENT;
        let kinds = format!("'{qp}Fluent','{qp}FluentObservation','{qp}Observable'");
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS fluent_type_codes (id INTEGER PRIMARY KEY, value BLOB NOT NULL UNIQUE);
             CREATE TABLE IF NOT EXISTS fluent_write_permits (tx INTEGER NOT NULL, g INTEGER NOT NULL, e INTEGER NOT NULL, PRIMARY KEY(tx,g,e));
             CREATE TABLE IF NOT EXISTS fluent_origin_receipts (g INTEGER NOT NULL, e INTEGER NOT NULL, tx INTEGER NOT NULL, proof BLOB NOT NULL, PRIMARY KEY(g,e));
             CREATE TABLE IF NOT EXISTS share_fluent_carriers (share_id TEXT PRIMARY KEY, manifest TEXT NOT NULL);
             CREATE TRIGGER IF NOT EXISTS fluent_origin_insert BEFORE INSERT ON fluent_origin_receipts
             WHEN NOT EXISTS(SELECT 1 FROM fluent_write_permits WHERE tx=NEW.tx AND g=NEW.g AND e=NEW.e)
             BEGIN SELECT RAISE(ABORT,'fluent origin receipt requires native admission'); END;
             CREATE TRIGGER IF NOT EXISTS fluent_origin_update BEFORE UPDATE ON fluent_origin_receipts
             BEGIN SELECT RAISE(ABORT,'fluent origin receipt cannot change'); END;
             CREATE TRIGGER IF NOT EXISTS fluent_origin_delete BEFORE DELETE ON fluent_origin_receipts
             BEGIN SELECT RAISE(ABORT,'fluent origin receipt cannot be deleted'); END;
             CREATE TRIGGER IF NOT EXISTS fluent_term_code AFTER INSERT ON terms WHEN NEW.iri IN ({kinds}) BEGIN
               INSERT OR IGNORE INTO fluent_type_codes(id,value) VALUES(NEW.id,{});
             END;
             INSERT OR IGNORE INTO fluent_type_codes(id,value) SELECT id,{} FROM terms WHERE iri IN ({kinds});",
            ref_blob("NEW.id"), ref_blob("id")
        ))?;
        let new_type = format!(
            "NEW.a=(SELECT id FROM terms WHERE iri='{}') AND NEW.v IN (SELECT value FROM fluent_type_codes)",
            crate::namespace::RDF_TYPE
        );
        let changed = [
            "e",
            "a",
            "v",
            "g",
            "tx",
            "valid_from",
            "valid_to",
            "op",
            "retracted_tx",
        ]
        .into_iter()
        .map(|column| format!("OLD.{column} IS NOT NEW.{column}"))
        .collect::<Vec<_>>()
        .join(" OR ");
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS fluent_fact_insert BEFORE INSERT ON facts
             WHEN EXISTS(SELECT 1 FROM fluent_type_codes) AND (({new_type}) OR ({}))
               AND NOT EXISTS(SELECT 1 FROM fluent_write_permits WHERE tx=NEW.tx AND g=NEW.g AND e=NEW.e)
             BEGIN SELECT RAISE(ABORT,'fluent immutable record requires native append'); END;
             CREATE TRIGGER IF NOT EXISTS fluent_fact_update BEFORE UPDATE ON facts
             WHEN ({changed}) AND (({}) OR ({}) OR ({new_type}))
             BEGIN SELECT RAISE(ABORT,'fluent immutable record cannot change'); END;
             CREATE TRIGGER IF NOT EXISTS fluent_fact_delete BEFORE DELETE ON facts WHEN ({})
             BEGIN SELECT RAISE(ABORT,'fluent immutable record cannot be deleted'); END;",
            protected("NEW"), protected("OLD"), protected("NEW"), protected("OLD")
        ))?;
        Ok(())
    }

    /// Only the normal native append builder reserves these exact record IDs.
    /// Permits live and disappear inside its existing transaction/savepoint.
    pub(super) fn permit_native_fluent_records(
        &self,
        tx: i64,
        graph: i64,
        datums: &[Datum],
    ) -> Result<()> {
        let identity = crate::transaction_auth::current_identity();
        let auth = self.transaction_auth(tx)?;
        let transaction = self.get_transaction(tx)?;
        if self.conn.is_autocommit()
            || tx != self.latest_tx_id()?
            || !auth.zip(identity).is_some_and(|(a, b)| {
                a.principal == b.principal
                    && a.credential_id == b.credential_id
                    && a.auth_class == b.auth_class
            })
            || transaction.as_ref().and_then(|t| t.source.as_deref()) != Some("quipu:fluent")
        {
            return Err(Error::InvalidValue(
                "fluent native transaction reservation missing".into(),
            ));
        }
        for entity in datums
            .iter()
            .map(|d| d.entity)
            .collect::<std::collections::BTreeSet<_>>()
        {
            self.conn.execute(
                "INSERT INTO fluent_write_permits(tx,g,e) VALUES(?1,?2,?3)",
                params![tx, graph, entity],
            )?;
        }
        Ok(())
    }

    pub(crate) fn fluent_origin_receipt(
        &self,
        graph: i64,
        event: i64,
    ) -> Result<Option<(Vec<u8>, i64)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT proof,tx FROM fluent_origin_receipts WHERE g=?1 AND e=?2",
                params![graph, event],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    pub(crate) fn persist_fluent_origin(
        &self,
        graph: i64,
        event: i64,
        tx: i64,
        proof: &[u8],
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO fluent_origin_receipts(g,e,tx,proof) VALUES(?1,?2,?3,?4)",
            params![graph, event, tx, proof],
        )?;
        Ok(())
    }

    pub(super) fn finish_native_fluent_records(&self, tx: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM fluent_write_permits WHERE tx=?1", [tx])?;
        Ok(())
    }
}
