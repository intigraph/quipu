pub use crate::store_replica::{Change, JournalState, Revision};
use ring::rand::SecureRandom as _;
use rusqlite::{OptionalExtension as _, params};

use super::catalog::{self, Table, quote};
use super::invalid;
use crate::{Result, Store};

fn cells(table: &Table, prefix: &str, indices: impl Iterator<Item = usize>) -> String {
    let fields: Vec<_> = indices.map(|i| {
        let value = format!("{prefix}.{}", quote(&table.columns[i].name));
        format!("json_object('kind',typeof({value}),'value',CASE typeof({value}) \
            WHEN 'blob' THEN hex({value}) WHEN 'real' THEN printf('%!.17g',{value}) ELSE {value} END)")
    }).collect();
    format!("json_array({})", fields.join(","))
}

fn triggers(table: &Table) -> Vec<(String, String)> {
    let key = |prefix| cells(table, prefix, table.key_indices().into_iter());
    let row = cells(table, "NEW", 0..table.columns.len());
    let put = format!(
        "INSERT INTO store_replication_changes(table_name,operation,key_data,row_data) \
        VALUES('{}','put',{},{});",
        table.name,
        key("NEW"),
        row
    );
    let delete = format!(
        "INSERT INTO store_replication_changes(table_name,operation,key_data,row_data) \
        VALUES('{}','delete',{},NULL);",
        table.name,
        key("OLD")
    );
    [
        ("insert", "INSERT", put.clone()),
        ("update", "UPDATE", format!("{delete}{put}")),
        ("delete", "DELETE", delete),
    ]
    .into_iter()
    .map(|(suffix, event, body)| {
        let name = format!("qsr_{}_{suffix}", table.name);
        let sql = format!(
            "CREATE TRIGGER {} AFTER {event} ON {} BEGIN {body} END",
            quote(&name),
            quote(&table.name)
        );
        (name, sql)
    })
    .collect()
}

/// Install the source journal atomically through an explicit local writer
/// action. Serving a snapshot/stream NEVER calls this function.
///
/// # Errors
/// Ownership refusal, an unclassified table/provider, incompatible schema,
/// altered/missing journal triggers, or any SQLite failure.
pub fn initialize(store: &Store) -> Result<JournalState> {
    store.ensure_ownership_writable()?;
    if !store.conn.is_autocommit() {
        return Err(invalid(
            "initialize the store journal outside application transactions",
        ));
    }
    let catalog = catalog::inspect(store)?;
    if has_state(store)? {
        return verify(store, &catalog);
    }
    store
        .conn
        .execute_batch("SAVEPOINT quipu_store_journal_init")?;
    let outcome = (|| {
        store.conn.execute_batch(
            "CREATE TABLE store_replication_state(id INTEGER PRIMARY KEY CHECK(id=1), \
                generation TEXT NOT NULL,catalog_digest TEXT NOT NULL); \
             CREATE TABLE store_replication_changes(revision INTEGER PRIMARY KEY AUTOINCREMENT, \
                table_name TEXT NOT NULL,operation TEXT NOT NULL CHECK(operation IN ('put','delete')), \
                key_data TEXT NOT NULL,row_data TEXT);"
        )?;
        let mut random = [0_u8; 16];
        ring::rand::SystemRandom::new()
            .fill(&mut random)
            .map_err(|_| invalid("could not mint store journal identity"))?;
        let state = JournalState {
            generation: hex::encode(random),
            catalog_digest: catalog::digest(&catalog)?,
        };
        store.conn.execute(
            "INSERT INTO store_replication_state VALUES(1,?1,?2)",
            params![state.generation, state.catalog_digest],
        )?;
        for table in &catalog {
            for (_, sql) in triggers(table) {
                store.conn.execute_batch(&sql)?;
            }
        }
        Ok(state)
    })();
    match outcome {
        Ok(state) => {
            store
                .conn
                .execute_batch("RELEASE quipu_store_journal_init")?;
            Ok(state)
        }
        Err(error) => {
            store.conn.execute_batch(
                "ROLLBACK TO quipu_store_journal_init; RELEASE quipu_store_journal_init",
            )?;
            Err(error)
        }
    }
}

fn has_state(store: &Store) -> Result<bool> {
    Ok(store
        .conn
        .prepare(
            "SELECT 1 FROM main.sqlite_schema \
        WHERE type='table' AND name='store_replication_state'",
        )?
        .exists([])?)
}

pub(super) fn verify(store: &Store, catalog: &[Table]) -> Result<JournalState> {
    if !has_state(store)? {
        return Err(invalid("source store journal is not initialized"));
    }
    let state = store.conn.query_row(
        "SELECT generation,catalog_digest FROM store_replication_state WHERE id=1",
        [],
        |r| {
            Ok(JournalState {
                generation: r.get(0)?,
                catalog_digest: r.get(1)?,
            })
        },
    )?;
    if state.catalog_digest != catalog::digest(catalog)? {
        return Err(invalid(
            "source store journal catalog changed; new admission required",
        ));
    }
    for table in catalog {
        for (name, sql) in triggers(table) {
            let actual: Option<String> = store
                .conn
                .query_row(
                    "SELECT sql FROM main.sqlite_schema WHERE type='trigger' AND name=?1",
                    [name],
                    |r| r.get(0),
                )
                .optional()?;
            if actual.as_deref() != Some(sql.as_str()) {
                return Err(invalid("source store journal trigger missing or altered"));
            }
        }
    }
    Ok(state)
}

/// Read one complete committed revision prefix, with a hard row cap. Never
/// truncate a logical source transaction to satisfy the cap: refuse instead.
/// A transport can spool/chunk this returned batch without multiple cursors.
///
/// # Errors
/// Missing/altered journal, a gap, invalid cursor, resource cap, or SQL/decoding.
pub fn read_revision(store: &Store, since: i64, max_changes: usize) -> Result<Revision> {
    read_bounded(store, since, max_changes.min(100_000), 8 * 1024 * 1024)
}

pub(super) fn read_bounded(
    store: &Store,
    since: i64,
    max_changes: usize,
    max_bytes: i64,
) -> Result<Revision> {
    if !store.conn.is_autocommit() {
        return Err(invalid("store journal reader requires its own snapshot"));
    }
    if !has_state(store)? {
        return Err(invalid("source store journal is not initialized"));
    }
    let read = store.conn.unchecked_transaction()?;
    let catalog = catalog::inspect(store)?;
    let journal = verify(store, &catalog)?;
    let through: i64 = store.conn.query_row(
        "SELECT coalesce(max(revision),0) FROM store_replication_changes",
        [],
        |r| r.get(0),
    )?;
    if since < 0 || since > through {
        return Err(invalid("invalid store journal cursor"));
    }
    let count =
        usize::try_from(through - since).map_err(|_| invalid("store journal batch too large"))?;
    if count > max_changes {
        return Err(invalid(
            "store journal batch exceeds resource cap; no partial publication",
        ));
    }
    // Measure stored bytes before copying or decoding any row into Rust. Text
    // length alone counts characters rather than bytes, hence the BLOB casts.
    let bytes: i64 = store.conn.query_row(
        "SELECT coalesce(sum(length(cast(table_name AS BLOB)) + \
         length(cast(operation AS BLOB)) + length(cast(key_data AS BLOB)) + \
         coalesce(length(cast(row_data AS BLOB)),0)),0) \
         FROM store_replication_changes WHERE revision>?1 AND revision<=?2",
        params![since, through],
        |r| r.get(0),
    )?;
    if bytes > max_bytes {
        return Err(invalid(
            "store journal byte cap exceeded; no partial publication",
        ));
    }
    let mut changes = Vec::with_capacity(count);
    let mut statement = store.conn.prepare(
        "SELECT revision,table_name,operation,key_data,row_data FROM store_replication_changes \
         WHERE revision>?1 AND revision<=?2 ORDER BY revision",
    )?;
    let mut rows = statement.query(params![since, through])?;
    while let Some(row) = rows.next()? {
        let key: String = row.get(3)?;
        let data: Option<String> = row.get(4)?;
        let change = Change {
            revision: row.get(0)?,
            table: row.get(1)?,
            operation: row.get(2)?,
            key: serde_json::from_str(&key).map_err(|error| super::json_error(&error))?,
            row: data
                .map(|s| serde_json::from_str(&s).map_err(|error| super::json_error(&error)))
                .transpose()?,
        };
        if change.revision != since + i64::try_from(changes.len()).unwrap_or(i64::MAX) + 1 {
            return Err(invalid("store journal has a gap; bootstrap required"));
        }
        changes.push(change);
    }
    if changes.len() != count {
        return Err(invalid("store journal has a gap; bootstrap required"));
    }
    let result = Revision {
        source: crate::replica_manifest::Source {
            store_id: store.store_id()?,
            log_epoch: store.log_epoch()?,
        },
        journal,
        since,
        through,
        source_tx: store.latest_tx_id()?,
        changes,
    };
    drop(rows);
    drop(statement);
    drop(read);
    Ok(result)
}
