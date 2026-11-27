//! Durable participant-commit records: the home journal, per-file intents,
//! the read/write fences derived from them and the exact payload codec
//! (aegis-n0wc.5 Phase 3). Split from `participant_commit.rs` for size only.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(super) const CONTRACT: &str = "quipu.participant-batch/v1";

pub(super) const JOURNAL: &str = "CREATE TABLE IF NOT EXISTS participant_journal (\
    batch_id TEXT PRIMARY KEY, batch_digest TEXT NOT NULL, members TEXT NOT NULL, \
    state TEXT NOT NULL CHECK(state IN ('preparing','committed','published','aborted')), \
    receipts TEXT)";

pub(super) const INTENTS: &str = "CREATE TABLE IF NOT EXISTS participant_intents (\
    batch_id TEXT PRIMARY KEY, batch_digest TEXT NOT NULL, home_file_id TEXT NOT NULL, \
    home_store_id TEXT NOT NULL, home_log_epoch TEXT NOT NULL, member INTEGER NOT NULL, \
    expected_head INTEGER NOT NULL, payload BLOB NOT NULL, intent_digest TEXT NOT NULL, \
    state TEXT NOT NULL CHECK(state IN ('prepared','applied','published','aborted')), \
    heads TEXT)";

/// One participant: an owner, its ordinary routed requests and its exact
pub(super) fn table_exists(store: &Store, name: &str) -> Result<bool> {
    Ok(store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |row| row.get(0),
    )?)
}

pub(super) fn pending_batch(store: &Store, states: &str) -> Result<Option<String>> {
    if !table_exists(store, "participant_intents")? {
        return Ok(None);
    }
    let sql = format!("SELECT batch_id FROM participant_intents WHERE state IN ({states}) LIMIT 1");
    match store.conn.query_row(&sql, [], |row| row.get(0)) {
        Ok(id) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Ordinary writes refuse while this file holds a prepared or applied intent.
pub(crate) fn check_write_fence(store: &Store) -> Result<()> {
    match pending_batch(store, "'prepared','applied'")? {
        Some(id) => Err(refuse(format!(
            "participant fenced by pending batch {id}; recover it first"
        ))),
        None => Ok(()),
    }
}

/// Reads refuse while this file holds applied facts the home has not published.
/// Call inside the read snapshot so the check and the data share one cut.
pub(crate) fn check_read_fence(store: &Store) -> Result<()> {
    match pending_batch(store, "'applied'")? {
        Some(id) => Err(refuse(format!(
            "file holds unpublished batch {id}; recover it first"
        ))),
        None => Ok(()),
    }
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn unhex(text: &str) -> Result<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return Err(refuse("damaged intent payload"));
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| refuse("damaged intent payload"))
        })
        .collect()
}

/// Exact, deterministic payload bytes for one participant's native delta.
pub(super) fn encode_payload(batch: &GraphSetBatch) -> Vec<u8> {
    let graphs: Vec<_> = batch
        .graphs
        .iter()
        .map(|(graph, datums)| {
            let datums: Vec<_> = datums
                .iter()
                .map(|d| {
                    serde_json::json!([
                        d.entity,
                        d.attribute,
                        hex(&d.value.to_bytes()),
                        d.valid_from,
                        d.valid_to,
                        d.op as i32
                    ])
                })
                .collect();
            serde_json::json!([graph, datums])
        })
        .collect();
    serde_json::json!({
        "timestamp": batch.timestamp,
        "actor": batch.actor,
        "source": batch.source,
        "graphs": graphs,
    })
    .to_string()
    .into_bytes()
}

pub(super) struct Decoded {
    pub(super) timestamp: String,
    pub(super) actor: Option<String>,
    pub(super) source: Option<String>,
    pub(super) graphs: Vec<(String, Vec<super::Datum>)>,
}

pub(super) fn decode_payload(bytes: &[u8]) -> Result<Decoded> {
    let bad = || refuse("damaged intent payload");
    let value: Value = serde_json::from_slice(bytes).map_err(|_| bad())?;
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    let mut graphs = Vec::new();
    for entry in value["graphs"].as_array().ok_or_else(bad)? {
        let graph = entry[0].as_str().ok_or_else(bad)?.to_owned();
        let mut datums = Vec::new();
        for d in entry[1].as_array().ok_or_else(bad)? {
            datums.push(super::Datum {
                entity: d[0].as_i64().ok_or_else(bad)?,
                attribute: d[1].as_i64().ok_or_else(bad)?,
                value: crate::types::Value::from_bytes(&unhex(d[2].as_str().ok_or_else(bad)?)?)
                    .map_err(|_| bad())?,
                valid_from: d[3].as_str().ok_or_else(bad)?.to_owned(),
                valid_to: d[4].as_str().map(str::to_owned),
                op: i32::try_from(d[5].as_i64().ok_or_else(bad)?)
                    .ok()
                    .and_then(crate::types::Op::from_i32)
                    .ok_or_else(bad)?,
            });
        }
        graphs.push((graph, datums));
    }
    Ok(Decoded {
        timestamp: text("timestamp").ok_or_else(bad)?,
        actor: text("actor"),
        source: text("source"),
        graphs,
    })
}

/// Participant identity recorded in the home journal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Recorded {
    pub(super) file_id: String,
    pub(super) store_id: String,
    pub(super) log_epoch: String,
}

pub(super) fn recorded(owner: &GraphSetOwner) -> Recorded {
    let route = owner.owner.catalog_route();
    Recorded {
        file_id: route.file_id.clone(),
        store_id: route.store_id.clone(),
        log_epoch: route.log_epoch.clone(),
    }
}

pub(super) fn intent_digest(
    batch_digest: &str,
    member: usize,
    head: i64,
    payload: &[u8],
) -> String {
    let mut bytes = format!("{CONTRACT}\n{batch_digest}\n{member}\n{head}\n").into_bytes();
    bytes.extend_from_slice(payload);
    sha256(&bytes)
}
/// Durable write on one participant file through its ownership seam.
pub(super) fn durable<T>(
    store: &mut Store,
    work: impl FnOnce(&mut Store) -> Result<T>,
) -> Result<T> {
    store.ownership_operation(|store| {
        store.conn.execute_batch(JOURNAL)?;
        store.conn.execute_batch(INTENTS)?;
        work(store)
    })
}

pub(super) struct JournalRow {
    pub(super) digest: String,
    pub(super) members: Vec<Recorded>,
    pub(super) state: String,
    pub(super) receipts: Option<String>,
}

pub(super) fn journal(store: &Store, batch_id: &str) -> Result<Option<JournalRow>> {
    if !table_exists(store, "participant_journal")? {
        return Ok(None);
    }
    let row = store.conn.query_row(
        "SELECT batch_digest, members, state, receipts FROM participant_journal WHERE batch_id=?1",
        [batch_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        },
    );
    match row {
        Ok((digest, members, state, receipts)) => Ok(Some(JournalRow {
            digest,
            members: serde_json::from_str(&members).map_err(|_| refuse("damaged journal"))?,
            state,
            receipts,
        })),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) struct IntentRow {
    pub(super) batch_digest: String,
    pub(super) home: Recorded,
    pub(super) member: i64,
    pub(super) expected_head: i64,
    pub(super) payload: Vec<u8>,
    pub(super) digest: String,
    pub(super) state: String,
    pub(super) heads: Option<String>,
}

pub(super) fn intent(store: &Store, batch_id: &str) -> Result<Option<IntentRow>> {
    if !table_exists(store, "participant_intents")? {
        return Ok(None);
    }
    let row = store.conn.query_row(
        "SELECT batch_digest, home_file_id, home_store_id, home_log_epoch, member, \
         expected_head, payload, intent_digest, state, heads \
         FROM participant_intents WHERE batch_id=?1",
        [batch_id],
        |row| {
            Ok(IntentRow {
                batch_digest: row.get(0)?,
                home: Recorded {
                    file_id: row.get(1)?,
                    store_id: row.get(2)?,
                    log_epoch: row.get(3)?,
                },
                member: row.get(4)?,
                expected_head: row.get(5)?,
                payload: row.get(6)?,
                digest: row.get(7)?,
                state: row.get(8)?,
                heads: row.get(9)?,
            })
        },
    );
    match row {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}
