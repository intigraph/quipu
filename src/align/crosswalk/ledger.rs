//! Internal append-only local acceptance events, never RDF or imported receipts.
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::*;

const INIT: &str = "CREATE TABLE IF NOT EXISTS mapping_acceptance_plans (
 sha256 TEXT PRIMARY KEY, plan_json TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS mapping_acceptance_events (
 id INTEGER PRIMARY KEY, installation TEXT NOT NULL, row_key TEXT NOT NULL,
 disposition TEXT NOT NULL, execution_uid INTEGER NOT NULL,
 declared_by TEXT NOT NULL, timestamp TEXT NOT NULL, plan_sha256 TEXT NOT NULL,
 supersedes INTEGER REFERENCES mapping_acceptance_events(id));
 CREATE INDEX IF NOT EXISTS mapping_acceptance_latest ON mapping_acceptance_events(installation,row_key,id);";

/// Selecting a source cannot call this action. It requires the separate local verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Disposition {
    Accept,
    Reject,
    Defer,
}

fn uid() -> Result<u32> {
    #[cfg(unix)]
    {
        Ok(rustix::process::geteuid().as_raw())
    }
    #[cfg(not(unix))]
    {
        Err(invalid(
            "local acceptance custody is not available on this platform",
        ))
    }
}

/// The caller resolves this connection locally and holds its writer reservation.
/// The installation argument comes from `store_id` or the private Git ledger identity,
/// never from the proposed file. Table initialization is explicit-decision only.
/// This library primitive is not a remote or serialized-receipt admission path.
///
/// # Errors
/// Wrong installation, ineligible/unknown row, absent local custody or SQL failure.
#[allow(clippy::too_many_arguments)]
pub fn decide(
    conn: &Connection,
    installation: &str,
    plan: &Plan,
    ordinal: usize,
    disposition: Disposition,
    declared_by: &str,
    timestamp: &str,
) -> Result<i64> {
    if installation != plan.context.installation
        || declared_by.trim().is_empty()
        || timestamp.trim().is_empty()
    {
        return Err(invalid(
            "local acceptance installation/actor/time binding missing",
        ));
    }
    let row = plan
        .rows
        .iter()
        .find(|r| r.ordinal == ordinal)
        .ok_or_else(|| invalid("unknown mapping row"))?;
    if disposition == Disposition::Accept && !row.identity_eligible {
        return Err(invalid(
            "only positive exact/sameAs rows can be accepted as identity",
        ));
    }
    let execution_uid = uid()?;
    let body = serde_json::to_string(plan).map_err(invalid)?;
    let plan_sha256 = plan.sha256()?;
    let row_key = Plan::row_key(&plan_sha256, row)?;
    conn.execute_batch("SAVEPOINT quipu_mapping_decision")?;
    let result = (|| {
        conn.execute_batch(INIT)?;
        conn.execute(
            "INSERT OR IGNORE INTO mapping_acceptance_plans (sha256,plan_json) VALUES (?1,?2)",
            params![plan_sha256, body],
        )?;
        let supersedes: Option<i64> = conn.query_row("SELECT id FROM mapping_acceptance_events WHERE installation=?1 AND row_key=?2 ORDER BY id DESC LIMIT 1", params![installation, row_key], |r| r.get(0)).optional()?;
        conn.execute("INSERT INTO mapping_acceptance_events (installation,row_key,disposition,execution_uid,declared_by,timestamp,plan_sha256,supersedes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![installation, row_key, format!("{disposition:?}"), execution_uid, declared_by, timestamp, plan_sha256, supersedes])?;
        Ok(conn.last_insert_rowid())
    })();
    match result {
        Ok(id) => {
            crate::store::release::release(conn, "quipu_mapping_decision")?;
            Ok(id)
        }
        Err(error) => {
            let _ = crate::store::release::rollback_release(conn, "quipu_mapping_decision");
            Err(error)
        }
    }
}

/// Refuse pending/deferred rows; rejection explicitly excludes a row. An error
/// is not an empty ledger. Source receipt fields are not an argument here.
/// The adapter supplies a freshly recomputed plan and locally resolved ledger
/// while holding input/decision publication custody. This does not lock inputs.
///
/// # Errors
/// Missing/stale/deferred decision, wrong custody, corrupt event or SQL failure.
pub fn verify(
    conn: &Connection,
    installation: &str,
    plan: &Plan,
) -> Result<BTreeMap<String, String>> {
    // Paired test-only naive verifier: recomputable proposal fields suffice.
    // No production build contains this branch or recognizes this variable.
    #[cfg(test)]
    if std::env::var_os("QUIPU_TEST_CROSSWALK_RECEIPT_MUTANT").is_some() {
        return Ok(plan
            .rows
            .iter()
            .filter(|row| row.identity_eligible)
            .map(|row| (row.incoming.clone(), row.retained.clone()))
            .collect());
    }
    if installation != plan.context.installation {
        return Err(invalid(
            "mapping acceptance belongs to another installation",
        ));
    }
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='mapping_acceptance_events')", [], |r| r.get(0))?;
    let plan_sha256 = plan.sha256()?;
    if exists {
        let body: Option<String> = conn
            .query_row(
                "SELECT plan_json FROM mapping_acceptance_plans WHERE sha256=?1",
                [&plan_sha256],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(body) = body {
            let recorded: Plan = serde_json::from_str(&body).map_err(invalid)?;
            if recorded != *plan {
                return Err(invalid("local acceptance plan changed"));
            }
        } else if !plan.rows.is_empty() {
            return Err(invalid("mapping plan pending local decision"));
        }
    }
    let mut out = BTreeMap::new();
    for row in &plan.rows {
        let event = if exists {
            conn.query_row("SELECT disposition, execution_uid, plan_sha256 FROM mapping_acceptance_events WHERE installation=?1 AND row_key=?2 ORDER BY id DESC LIMIT 1", params![installation, Plan::row_key(&plan_sha256, row)?], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, String>(2)?))).optional()?
        } else {
            None
        };
        let Some((disposition, execution_uid, recorded_sha256)) = event else {
            return Err(invalid(format!(
                "mapping row {} pending local decision",
                row.ordinal
            )));
        };
        if recorded_sha256 != plan_sha256 || execution_uid != uid()? {
            return Err(invalid(
                "local acceptance context or execution custody changed",
            ));
        }
        match disposition.as_str() {
            "Accept" if row.identity_eligible => {
                out.insert(row.incoming.clone(), row.retained.clone());
            }
            "Reject" => {}
            _ => {
                return Err(invalid(format!(
                    "mapping row {} deferred or ineligible",
                    row.ordinal
                )));
            }
        }
    }
    Ok(out)
}
