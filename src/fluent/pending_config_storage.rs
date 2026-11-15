//! Pending content travels; native acceptance evidence is receiver-local.
use super::{Approval, MAX_BYTES, Pending, invalid};
use crate::{Result, Store};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS fluent_pending_configs(id TEXT PRIMARY KEY, request BLOB NOT NULL);
         CREATE TABLE IF NOT EXISTS fluent_config_receipts(g INTEGER NOT NULL,e INTEGER NOT NULL,tx INTEGER NOT NULL,request BLOB NOT NULL,approval BLOB NOT NULL,PRIMARY KEY(g,e));
         CREATE TRIGGER IF NOT EXISTS fluent_config_insert BEFORE INSERT ON fluent_config_receipts
         WHEN NOT EXISTS(SELECT 1 FROM fluent_write_permits WHERE tx=NEW.tx AND g=NEW.g AND e=NEW.e)
         BEGIN SELECT RAISE(ABORT,'config receipt requires native acceptance'); END;
         CREATE TRIGGER IF NOT EXISTS fluent_config_update BEFORE UPDATE ON fluent_config_receipts
         BEGIN SELECT RAISE(ABORT,'config receipt cannot change'); END;
         CREATE TRIGGER IF NOT EXISTS fluent_config_delete BEFORE DELETE ON fluent_config_receipts
         BEGIN SELECT RAISE(ABORT,'config receipt cannot be deleted'); END;"
    )?;
    Ok(())
}

/// Transfer count and per-request sizes are bounded before cloning wire data.
pub(crate) fn validate_requests(requests: &[String]) -> Result<()> {
    if requests.len() > 64 {
        return Err(invalid("too many pending config requests"));
    }
    for raw in requests {
        Pending::decode(raw.as_bytes())?;
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Receipt {
    pub tx: i64,
    pub request: Vec<u8>,
    pub approval: Vec<u8>,
}

type RawReceipt = (i64, Option<Vec<u8>>, Option<Vec<u8>>);

impl Pending {
    pub(crate) fn retain(&self, store: &Store) -> Result<()> {
        store.ensure_ownership_writable()?;
        store.conn.execute("INSERT INTO fluent_pending_configs(id,request) VALUES(?1,?2) ON CONFLICT(id) DO NOTHING", params![self.id(), self.bytes()])?;
        let prior = Self::load(store, &self.id())?
            .ok_or_else(|| invalid("pending config retention failed"))?;
        if prior.bytes() != self.bytes() {
            return Err(invalid("conflicting pending config identity"));
        }
        Ok(())
    }

    pub(crate) fn load(store: &Store, id: &str) -> Result<Option<Self>> {
        let raw: Option<Option<Vec<u8>>> = store.conn.query_row(
            "SELECT CASE WHEN length(request)<=?2 THEN request ELSE NULL END FROM fluent_pending_configs WHERE id=?1",
            params![id, MAX_BYTES as i64], |row| row.get(0)).optional()?;
        match raw {
            None => Ok(None),
            Some(None) => Err(invalid("retained config exceeds bound")),
            Some(Some(bytes)) => {
                let pending = Self::decode(&bytes)?;
                if pending.id() != id {
                    return Err(invalid("retained config identity mismatch"));
                }
                Ok(Some(pending))
            }
        }
    }

    pub(crate) fn receipt(&self, store: &Store, graph: i64) -> Result<Option<Receipt>> {
        let Some(entity) = store.lookup(&self.registration().observable.name)? else {
            return Ok(None);
        };
        let raw: Option<RawReceipt> = store.conn.query_row(
            "SELECT tx,CASE WHEN length(request)<=65536 THEN request ELSE NULL END,CASE WHEN length(approval)<=16384 THEN approval ELSE NULL END FROM fluent_config_receipts WHERE g=?1 AND e=?2",
            params![graph,entity], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        raw.map(|(tx, request, approval)| {
            Ok(Receipt {
                tx,
                request: request.ok_or_else(|| invalid("config receipt request exceeds bound"))?,
                approval: approval
                    .ok_or_else(|| invalid("config receipt approval exceeds bound"))?,
            })
        })
        .transpose()
    }

    pub(crate) fn persist_receipt(
        &self,
        store: &Store,
        graph: i64,
        tx: i64,
        approval: &Approval,
    ) -> Result<()> {
        let entity = store
            .lookup(&self.registration().observable.name)?
            .ok_or_else(|| invalid("accepted observable missing"))?;
        store.conn.execute(
            "INSERT INTO fluent_config_receipts(g,e,tx,request,approval) VALUES(?1,?2,?3,?4,?5)",
            params![graph, entity, tx, self.bytes(), approval.bytes()?],
        )?;
        Ok(())
    }
}
