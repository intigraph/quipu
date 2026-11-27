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

/// Dependency pins (S4): this file's record that another catalog file relies
/// on "entity a `type_iri` in graph". Metadata, never facts or a tx head.
pub(super) const PINS: &str = "CREATE TABLE IF NOT EXISTS dependency_pins (\
    graph TEXT NOT NULL, entity TEXT NOT NULL, type_iri TEXT NOT NULL, \
    dependent_file_id TEXT NOT NULL, \
    PRIMARY KEY (graph, entity, type_iri, dependent_file_id))";

/// Subclass-axiom pins (slice B): this file's record that a dependent file's
/// sh:class satisfaction for `via` relied on "class rdfs:subClassOf parent"
/// in graph. Released with `via`'s type pins.
pub(super) const SUBCLASS_PINS: &str = "CREATE TABLE IF NOT EXISTS subclass_pins (\
    graph TEXT NOT NULL, class TEXT NOT NULL, parent TEXT NOT NULL, via TEXT NOT NULL, \
    dependent_file_id TEXT NOT NULL, \
    PRIMARY KEY (graph, class, parent, via, dependent_file_id))";

/// Slice B audit records (dearing c71): post-commit facts about a judged
/// member, never a fence. kind = 'shapes-delta' (judged under X, current Y)
/// or 'verify-failed' (the applied member does not verify under the carried
/// shapes: a corrupt intent or a replay bug; loud, stays applied).
pub(super) const AUDIT: &str = "CREATE TABLE IF NOT EXISTS participant_audit (\
    batch_id TEXT NOT NULL, kind TEXT NOT NULL, judged_sha256 TEXT NOT NULL, \
    current_sha256 TEXT, detail TEXT NOT NULL, \
    PRIMARY KEY (batch_id, kind))";

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

/// Pin changes one participant applies with its intent (S4): pins it records
/// for a validated batch, and (entity, dependent file) pairs a release batch
/// proved no longer referenced, whose pins it drops.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct PinChanges {
    pub(super) add: Vec<super::deps::DependencyPin>,
    pub(super) release: Vec<(String, String)>,
    /// Slice B: subclass-axiom pins this (source) file records.
    pub(super) axioms: Vec<super::deps::SubclassPin>,
    /// Slice B: derived type context this (member) file's apply is judged with.
    pub(super) context: Vec<[String; 3]>,
    /// Slice B (malcolm C1, dearing c71): (sha256, exact bytes) of the write
    /// shapes this member was judged by at PREPARE. Re-checked before the
    /// COMMIT decision; carried so recovery can audit under them after a reload.
    pub(super) shapes: Option<(String, String)>,
}

impl PinChanges {
    fn is_empty(&self) -> bool {
        self.add.is_empty()
            && self.release.is_empty()
            && self.axioms.is_empty()
            && self.context.is_empty()
            && self.shapes.is_none()
    }
}

/// Exact, deterministic payload bytes for one participant's native delta and
/// its pin changes. Empty changes emit exactly the bytes a plain batch always
/// had, so existing intents and digests are unchanged.
pub(super) fn encode_payload_with(batch: &GraphSetBatch, pins: &PinChanges) -> Vec<u8> {
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
    let mut value = serde_json::json!({
        "timestamp": batch.timestamp,
        "actor": batch.actor,
        "source": batch.source,
        "graphs": graphs,
    });
    if !pins.is_empty() {
        let add: Vec<_> = pins
            .add
            .iter()
            .map(|p| serde_json::json!([p.graph, p.entity, p.type_iri, p.dependent_file_id]))
            .collect();
        value["pins"] = serde_json::json!({"add": add, "release": pins.release});
        // Slice B keys appear only when used, so S4 payload bytes are unchanged.
        if !pins.axioms.is_empty() {
            value["pins"]["axioms"] = serde_json::json!(
                pins.axioms
                    .iter()
                    .map(|a| [&a.graph, &a.class, &a.parent, &a.via, &a.dependent_file_id])
                    .collect::<Vec<_>>()
            );
        }
        if !pins.context.is_empty() {
            value["pins"]["context"] = serde_json::json!(pins.context);
        }
        if let Some((sha, bytes)) = &pins.shapes {
            value["pins"]["shapes_sha256"] = serde_json::json!(sha);
            value["pins"]["shapes"] = serde_json::json!(bytes);
        }
    }
    value.to_string().into_bytes()
}

pub(super) struct Decoded {
    pub(super) timestamp: String,
    pub(super) actor: Option<String>,
    pub(super) source: Option<String>,
    pub(super) graphs: Vec<(String, Vec<super::Datum>)>,
    pub(super) pins: PinChanges,
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
    let mut pins = PinChanges::default();
    if let Some(changes) = value.get("pins") {
        let field = |row: &Value, i: usize| row[i].as_str().map(str::to_owned).ok_or_else(bad);
        for row in changes["add"].as_array().ok_or_else(bad)? {
            pins.add.push(super::deps::DependencyPin {
                graph: field(row, 0)?,
                entity: field(row, 1)?,
                type_iri: field(row, 2)?,
                dependent_file_id: field(row, 3)?,
            });
        }
        for row in changes["release"].as_array().ok_or_else(bad)? {
            pins.release.push((field(row, 0)?, field(row, 1)?));
        }
        for row in changes
            .get("axioms")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            pins.axioms.push(super::deps::SubclassPin {
                graph: field(row, 0)?,
                class: field(row, 1)?,
                parent: field(row, 2)?,
                via: field(row, 3)?,
                dependent_file_id: field(row, 4)?,
            });
        }
        for row in changes
            .get("context")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            pins.context
                .push([field(row, 0)?, field(row, 1)?, field(row, 2)?]);
        }
        if let Some(sha) = changes.get("shapes_sha256") {
            // Bytes may be absent or altered: recovery still applies, and its
            // audit reports the shapes it could not verify under.
            let bytes = changes
                .get("shapes")
                .and_then(Value::as_str)
                .unwrap_or_default();
            pins.shapes = Some((sha.as_str().ok_or_else(bad)?.to_owned(), bytes.to_owned()));
        }
    }
    Ok(Decoded {
        timestamp: text("timestamp").ok_or_else(bad)?,
        actor: text("actor"),
        source: text("source"),
        graphs,
        pins,
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
        store.conn.execute_batch(PINS)?;
        store.conn.execute_batch(SUBCLASS_PINS)?;
        store.conn.execute_batch(AUDIT)?;
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

/// Digest of every `graphs` row: a participant apply must leave it unchanged.
pub(super) fn graphs_digest(store: &Store) -> Result<String> {
    let mut stmt = store.conn.prepare("SELECT * FROM graphs ORDER BY g")?;
    let width = stmt.column_count();
    let rows: Vec<String> = stmt
        .query_map([], |row| {
            (0..width)
                .map(|i| Ok(format!("{:?}", row.get_ref(i)?)))
                .collect::<rusqlite::Result<Vec<_>>>()
                .map(|cells| cells.join("\u{1f}"))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(sha256(rows.join("\n").as_bytes()))
}

/// The applied-but-unpublished batch this file holds, if any: what a composed
/// reader needs to pick this file's publication cut (Phase 3 S3).
pub(crate) struct AppliedIntent {
    pub(crate) batch_id: String,
    pub(crate) home_file_id: String,
    pub(crate) home_store_id: String,
    pub(crate) home_log_epoch: String,
    /// This file's head before the batch: its last published local head.
    pub(crate) expected_head: i64,
}

pub(crate) fn applied_intent(store: &Store) -> Result<Option<AppliedIntent>> {
    if !table_exists(store, "participant_intents")? {
        return Ok(None);
    }
    let mut statement = store.conn.prepare(
        "SELECT batch_id, home_file_id, home_store_id, home_log_epoch, expected_head \
         FROM participant_intents WHERE state = 'applied' LIMIT 2",
    )?;
    let mut rows: Vec<AppliedIntent> = statement
        .query_map([], |row| {
            Ok(AppliedIntent {
                batch_id: row.get(0)?,
                home_file_id: row.get(1)?,
                home_store_id: row.get(2)?,
                home_log_epoch: row.get(3)?,
                expected_head: row.get(4)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    if rows.len() > 1 {
        return Err(refuse("file holds more than one unpublished batch"));
    }
    Ok(rows.pop())
}

/// Whether the home journal on `conn` records `batch_id` as PUBLISHED, after
/// verifying `conn` is the recorded home. `None`: no journal row (UNKNOWN).
pub(crate) fn home_published(
    conn: &rusqlite::Connection,
    intent: &AppliedIntent,
) -> Result<Option<bool>> {
    let identity: (String, String) = conn.query_row(
        "SELECT (SELECT store_id FROM store_identity WHERE id = 1), \
                (SELECT epoch FROM log_epoch WHERE id = 1)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if identity != (intent.home_store_id.clone(), intent.home_log_epoch.clone()) {
        return Err(refuse(
            "home participant identity differs; the old cut is not provable",
        ));
    }
    let present: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' \
         AND name='participant_journal')",
        [],
        |row| row.get(0),
    )?;
    if !present {
        return Ok(None);
    }
    match conn.query_row(
        "SELECT state FROM participant_journal WHERE batch_id = ?1",
        [&intent.batch_id],
        |row| row.get::<_, String>(0),
    ) {
        Ok(state) => Ok(Some(state == "published")),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// The caller's verified, unsigned and unscoped principal.
pub(super) fn principal() -> Result<crate::transaction_auth::Identity> {
    let identity = crate::transaction_auth::current_identity()
        .ok_or_else(|| refuse("a verified principal is required"))?;
    if crate::transaction_auth::current_attestation().is_some() {
        return Err(refuse("signed participant batches are not qualified"));
    }
    if crate::writer_scope::is_scoped_request() {
        return Err(refuse("scoped participant batches are not qualified"));
    }
    Ok(identity)
}
