//! Shared manifest schema and writer for every pack producer.
use super::Manifest;
use crate::Result;

/// The `pack_manifest` DDL. One row, by construction.
/// Create `pack_manifest` and write the single row, for every pack producer.
///
/// Shared so the published pack and the `--full` pack cannot drift apart on
/// the manifest schema — the column list appears once. Same reasoning as
/// `enforce_pack_destination`: the third producer is the one that forgets.
///
/// # Errors
/// Propagates SQLite errors from the create or the insert.
pub(crate) fn write_manifest(conn: &rusqlite::Connection, manifest: &Manifest) -> Result<()> {
    conn.execute_batch(MANIFEST_SQL)?;
    conn.execute(
        "INSERT OR REPLACE INTO pack_manifest \
         (id, pack_format, name, version, term_space, content_hash, created_at, \
          source_graph, producer, counts, destination) \
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            manifest.pack_format,
            manifest.name,
            manifest.version,
            manifest.term_space,
            manifest.content_hash,
            manifest.created_at,
            manifest.source_graph,
            manifest.producer,
            manifest.counts,
            manifest.destination.as_ref().map(|d| match d {
                crate::share_scrub::ShareDestination::Internal => "internal".into(),
                crate::share_scrub::ShareDestination::Outward => "outward".into(),
                crate::share_scrub::ShareDestination::Audience(_) =>
                    serde_json::to_string(d).expect("destination JSON"),
            }),
        ],
    )?;
    Ok(())
}

pub(crate) const MANIFEST_SQL: &str = "CREATE TABLE IF NOT EXISTS pack_manifest (
     id           INTEGER PRIMARY KEY CHECK (id = 1),
     pack_format  TEXT NOT NULL,
     name         TEXT NOT NULL,
     version      TEXT NOT NULL,
     term_space   INTEGER NOT NULL,
     content_hash TEXT NOT NULL,
     created_at   TEXT NOT NULL,
     source_graph TEXT NOT NULL,
     producer     TEXT NOT NULL,
     counts       TEXT NOT NULL,
     -- Nullable ON PURPOSE: a pack written before aegis-9f899e has no row for
     -- this and must read back as UNKNOWN rather than as either destination.
     destination  TEXT
 );";
