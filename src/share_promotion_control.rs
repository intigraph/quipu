//! Paired, store/head-bound ROOT promotion and exact source-claim inverse.
//! Transport callers must supply the authenticated principal, never a body actor.
use crate::share_import::{PromoteImportRequest, PromoteImportResult};
use crate::{Datum, Error, Op, Result, Store, Value};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Explicit conditional ROOT promotion; no graph move or caller actor override.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Promote {
    pub store_id: String,
    pub expected_head: i64,
    pub share_id: String,
}

/// An identifier receipt, not a credential or permission to write.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub store_id: String,
    pub share_id: String,
    pub promotion_tx: i64,
}

/// Promotion result and the exact inverse's identifiers.
#[derive(Debug, Serialize)]
pub struct Promoted {
    pub promotion: PromoteImportResult,
    pub receipt: Receipt,
}

/// Actual inverse transaction and number of source claims retracted.
#[derive(Debug, Serialize)]
pub struct Retracted {
    pub tx_id: i64,
    pub claims: usize,
}

fn reserve(store: &mut Store, store_id: &str, head: i64, principal: &str) -> Result<()> {
    if principal.trim().is_empty() || head < 0 {
        return Err(Error::InvalidValue(
            "authenticated principal and nonnegative head required".into(),
        ));
    }
    store
        .conn
        .execute("UPDATE transactions SET id=id WHERE 0", [])?;
    if store.store_id()? != store_id || store.latest_tx_id()? != head {
        return Err(Error::ConditionalWriteConflict);
    }
    crate::time::check_request_deadline()
}

/// Promote through the normal review, deadline and governance gates.
///
/// # Errors
/// Refuses an identity/head mismatch, an unauthenticated caller, a query-bearing
/// promotion or a promotion that creates additional transactions. Such effects
/// have no exact fact-only inverse and the entire savepoint is rolled back.
pub fn promote(store: &mut Store, input: &Promote, principal: &str, ts: &str) -> Result<Promoted> {
    super::atomic(store, |store| {
        reserve(store, &input.store_id, input.expected_head, principal)?;
        let request = PromoteImportRequest {
            share_id: input.share_id.clone(),
            actor: None,
            target_graph: None,
            retire_from: None,
            dry_run: false,
        };
        let promotion = super::promote_transition(store, &request, ts, Some(principal))?;
        if promotion.queries.is_some()
            || promotion.tx_id <= input.expected_head
            || store.latest_tx_id()? != promotion.tx_id
        {
            return Err(Error::InvalidValue(
                "promotion has no single-transaction fact-only inverse".into(),
            ));
        }
        let receipt = Receipt {
            store_id: input.store_id.clone(),
            share_id: input.share_id.clone(),
            promotion_tx: promotion.tx_id,
        };
        let binding = serde_json::json!({"principal": principal, "receipt": receipt});
        store.conn.execute("INSERT INTO events(type,ts,subject,group_id,tx_id,payload) VALUES('import.promotion_receipted',?1,?2,NULL,?3,?4)",
            params![ts, receipt.share_id, receipt.promotion_tx, binding.to_string()])?;
        crate::time::check_request_deadline()?;
        Ok(Promoted { promotion, receipt })
    })
}

/// Retract ONLY the ROOT source claims asserted by the receipted promotion.
///
/// The writer reservation precedes every identity, head, transaction and fact
/// read. A later transaction (including a prior inverse) refuses without writes.
/// Other sources' identical facts and every staging fact remain untouched.
///
/// # Errors
/// Refuses a wrong store/share/principal, an intervening transaction, an empty
/// inverse, a deadline, or any normal retraction governance/SHACL refusal.
pub fn retract(
    store: &mut Store,
    receipt: &Receipt,
    principal: &str,
    ts: &str,
) -> Result<Retracted> {
    super::atomic(store, |store| {
        reserve(store, &receipt.store_id, receipt.promotion_tx, principal)?;
        // Validate the share identifier through the normal parser and bind the
        // source exactly. Body-supplied actors are never accepted on this path.
        crate::share_import::staging_graph(&receipt.share_id, false)?;
        let source =
            crate::share_import::provenance_source("share-promotion", &receipt.share_id, None);
        let transaction: Option<(Option<String>, Option<String>)> = store
            .conn
            .query_row(
                "SELECT actor,source FROM transactions WHERE id=?1",
                [receipt.promotion_tx],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if transaction != Some((Some(principal.to_owned()), Some(source.clone()))) {
            return Err(Error::InvalidValue(
                "promotion receipt does not match authenticated transaction".into(),
            ));
        }
        // Only this paired path issues a receipt. Ordinary historical promotion
        // could also have installed queries and is not a fact-only inverse.
        let binding = serde_json::json!({"principal": principal, "receipt": receipt});
        let bindings: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM events WHERE type='import.promotion_receipted' AND subject=?1 AND tx_id=?2 AND payload=?3",
            params![receipt.share_id, receipt.promotion_tx, binding.to_string()], |r| r.get(0))?;
        if bindings != 1 {
            return Err(Error::InvalidValue(
                "missing or ambiguous paired promotion receipt".into(),
            ));
        }
        // Seek exact immutable transaction membership, never scan whole ROOT.
        let rows = {
            let mut stmt = store.conn.prepare(
                "SELECT e,a,v FROM facts WHERE tx=?1 AND g=0 AND op=1 AND valid_to IS NULL",
            )?;
            stmt.query_map([receipt.promotion_tx], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
        };
        if rows.is_empty() {
            return Err(Error::InvalidValue(
                "promotion has no active ROOT claims to retract".into(),
            ));
        }
        let datums = rows
            .into_iter()
            .map(|(entity, attribute, bytes)| {
                crate::time::check_request_deadline()?;
                Ok(Datum {
                    entity,
                    attribute,
                    value: Value::from_bytes(&bytes)?,
                    valid_from: ts.into(),
                    valid_to: None,
                    op: Op::Retract,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let tx_id = store.transact_to_graph_scoped(
            &datums,
            ts,
            Some(principal),
            Some("share-promotion-inverse"),
            0,
            Some(&source),
        )?;
        crate::time::check_request_deadline()?;
        // This is a retraction, not a review reset or a retry lease.
        store.conn.execute("INSERT INTO events(type,ts,subject,group_id,tx_id,payload) VALUES('import.promotion_retracted',?1,?2,NULL,?3,?4)",
            params![ts, receipt.share_id, tx_id, serde_json::to_string(receipt).map_err(|e| Error::Serialization(e.to_string()))?])?;
        Ok(Retracted {
            tx_id,
            claims: datums.len(),
        })
    })
}
