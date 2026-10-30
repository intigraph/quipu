use std::io::Write;

use sha2::{Digest, Sha256};

use super::catalog::{self, Cell, quote};
use super::{invalid, journal};
use crate::store_replica::{SnapshotHeader, SnapshotRow};
use crate::{Result, Store};

/// Completion receipt for a private logical snapshot spool. A partial output
/// has NO receipt and must never be signed, served or admitted as a bootstrap.
#[derive(Debug)]
pub struct SnapshotReceipt {
    /// All source identities and cursors read in one SQLite snapshot.
    pub header: SnapshotHeader,
    /// SHA-256 of every emitted UTF-8 JSON line, including trailing newlines.
    pub sha256: String,
    /// Number of complete table rows emitted, excluding the header.
    pub rows: u64,
    /// Total emitted bytes.
    pub bytes: u64,
}

fn emit(
    output: &mut impl Write,
    value: &impl serde::Serialize,
    hash: &mut Sha256,
    bytes: &mut u64,
    max_bytes: u64,
) -> Result<()> {
    let mut line = serde_json::to_vec(value).map_err(|error| super::json_error(&error))?;
    line.push(b'\n');
    if line.len() > 8 * 1024 * 1024 {
        return Err(invalid("logical snapshot record exceeds byte cap"));
    }
    let next = bytes
        .checked_add(line.len() as u64)
        .ok_or_else(|| invalid("logical snapshot byte counter overflow"))?;
    if next > max_bytes {
        return Err(invalid("logical snapshot exceeds total byte cap"));
    }
    output
        .write_all(&line)
        .map_err(|error| invalid(format!("logical snapshot output: {}", error.kind())))?;
    hash.update(&line);
    *bytes = next;
    Ok(())
}

/// Stream a logical cut to a caller-owned PRIVATE staging writer. No active
/// database/WAL file is copied, no graph cap is raised, and no journal is
/// initialized by this read. The caller publishes only after this function
/// returns a completion receipt and subsequent signature/admission checks.
///
/// # Errors
/// Uninitialized/altered journal, unclassified schema/provider, record/total
/// byte cap, invalid cells, output failure, or SQLite failure. Output on any
/// failure is incomplete and must be discarded by its owning caller.
pub fn write_snapshot(
    store: &Store,
    output: &mut impl Write,
    max_bytes: u64,
) -> Result<SnapshotReceipt> {
    if !store.conn.is_autocommit() {
        return Err(invalid(
            "logical snapshot requires its own read transaction",
        ));
    }
    let read = store.conn.unchecked_transaction()?;
    let catalog = catalog::inspect(store)?;
    let journal = journal::verify(store, &catalog)?;
    let header = SnapshotHeader {
        source: crate::replica_manifest::Source {
            store_id: store.store_id()?,
            log_epoch: store.log_epoch()?,
        },
        journal,
        revision: store.conn.query_row(
            "SELECT coalesce(max(revision),0) FROM store_replication_changes",
            [],
            |r| r.get(0),
        )?,
        source_tx: store.latest_tx_id()?,
        catalog,
    };
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut count = 0_u64;
    emit(output, &header, &mut hash, &mut bytes, max_bytes)?;
    for table in &header.catalog {
        let fields = table
            .columns
            .iter()
            .map(|c| quote(&c.name))
            .collect::<Vec<_>>();
        let order = table
            .key_indices()
            .iter()
            .map(|&i| fields[i].clone())
            .collect::<Vec<_>>();
        let sizes = fields
            .iter()
            .map(|c| format!("coalesce(length(cast({c} AS BLOB)),0)"))
            .collect::<Vec<_>>()
            .join("+");
        let mut statement = store.conn.prepare(&format!(
            "SELECT {sizes},{} FROM {} ORDER BY {}",
            fields.join(","),
            quote(&table.name),
            order.join(","),
        ))?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            // Refuse before allocating Rust strings/blobs. Encoding has its
            // own upper bound because JSON escaping/hex can expand input.
            if row.get::<_, i64>(0)? > 1024 * 1024 {
                return Err(invalid("logical snapshot source row exceeds byte cap"));
            }
            let cells = (0..fields.len())
                .map(|i| Cell::from_sql(row.get_ref(i + 1)?))
                .collect::<Result<Vec<_>>>()?;
            emit(
                output,
                &SnapshotRow {
                    table: table.name.clone(),
                    cells,
                },
                &mut hash,
                &mut bytes,
                max_bytes,
            )?;
            count = count
                .checked_add(1)
                .ok_or_else(|| invalid("logical snapshot row counter overflow"))?;
        }
    }
    output
        .flush()
        .map_err(|error| invalid(format!("logical snapshot output: {}", error.kind())))?;
    drop(read);
    Ok(SnapshotReceipt {
        header,
        sha256: hex::encode(hash.finalize()),
        rows: count,
        bytes,
    })
}
