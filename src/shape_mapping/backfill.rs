//! Bounded, resumable legacy registry identities. Store open only adds metadata.

use rusqlite::{OptionalExtension, params};

use super::{RegistryKind, canonical_shape_digest};
use crate::{Result, Store};

/// Maximum legacy Turtle bytes copied for one canonicalization attempt.
const MAX_TURTLE_BYTES: i64 = 1 << 20;

pub(super) fn migrate(conn: &rusqlite::Connection) -> Result<()> {
    for table in ["shapes", "ontologies"] {
        for column in ["digest", "digest_backfill_error"] {
            let present = conn
                .prepare(&format!(
                    "SELECT 1 FROM pragma_table_info('{table}') WHERE name=?1"
                ))?
                .exists([column])?;
            if !present {
                conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} TEXT"))?;
            }
        }
        conn.execute_batch(&format!(
            "CREATE INDEX IF NOT EXISTS idx_{table}_digest_pending ON {table}((valid_to IS NOT NULL),name,valid_from) \
             WHERE digest IS NULL AND digest_backfill_error IS NULL"
        ))?;
    }
    Ok(())
}

/// One private version snapshot; computation needs no connection or store lock.
#[derive(Debug)]
pub struct RegistryDigestWork {
    kind: RegistryKind,
    name: String,
    at: String,
    tx: Option<i64>,
    turtle: Option<String>,
}

/// A computed snapshot, applied only if its original version still matches.
#[derive(Debug)]
pub struct RegistryDigestResult {
    work: RegistryDigestWork,
    digest: Option<String>,
    error: Option<&'static str>,
}

impl RegistryDigestWork {
    /// Compute outside the store mutex. Unverifiable legacy versions stay NULL.
    pub fn compute(self) -> RegistryDigestResult {
        let (digest, error) = match self.turtle.as_deref() {
            Some(turtle) => match canonical_shape_digest(turtle) {
                Ok(digest) => (Some(digest), None),
                Err(_) => (None, Some("RDF parsing or canonicalization failed")),
            },
            None => (None, Some("legacy Turtle exceeds the 1 MiB backfill bound")),
        };
        RegistryDigestResult {
            work: self,
            digest,
            error,
        }
    }
}

/// Persisted state across both registries, including explicitly unpinnable rows.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RegistryDigestBackfillStatus {
    /// NULL identities not yet attempted.
    pub pending: i64,
    /// Versions with a verified canonical digest.
    pub complete: i64,
    /// NULL identities with a retained failure classification.
    pub failed: i64,
}

impl Store {
    /// Read at most one bounded legacy document using the pending partial index.
    pub fn next_registry_digest_work(&self) -> Result<Option<RegistryDigestWork>> {
        // Current pin availability comes first in BOTH registries, then history.
        // The leading partial-index expression keeps empty-priority probes bounded.
        for historical in [false, true] {
            for kind in [RegistryKind::Shapes, RegistryKind::Ontologies] {
                let row = self
                    .conn
                    .query_row(
                        &format!(
                            "SELECT name,valid_from,tx,CASE WHEN length(CAST(turtle AS BLOB))<=?1 \
                         THEN turtle ELSE NULL END FROM {} \
                         WHERE digest IS NULL AND digest_backfill_error IS NULL \
                         AND (valid_to IS NOT NULL)=?2 ORDER BY name,valid_from LIMIT 1",
                            kind.table()
                        ),
                        params![MAX_TURTLE_BYTES, historical],
                        |r| {
                            Ok(RegistryDigestWork {
                                kind,
                                name: r.get(0)?,
                                at: r.get(1)?,
                                tx: r.get(2)?,
                                turtle: r.get(3)?,
                            })
                        },
                    )
                    .optional()?;
                if row.is_some() {
                    return Ok(row);
                }
            }
        }
        Ok(None)
    }

    /// Atomically persist progress without rewriting registry content or history.
    /// A concurrent same-instant replacement cannot inherit an obsolete digest.
    pub fn apply_registry_digest_result(&self, result: RegistryDigestResult) -> Result<bool> {
        self.ensure_ownership_writable()?;
        let w = result.work;
        let updated = self.conn.execute(
            &format!(
                "UPDATE {} SET digest=?4,digest_backfill_error=?5 \
                 WHERE name=?1 AND valid_from=?2 AND tx IS ?3 \
                 AND digest IS NULL AND digest_backfill_error IS NULL \
                 AND ((?6 IS NOT NULL AND turtle=?6) OR \
                      (?6 IS NULL AND length(CAST(turtle AS BLOB))>?7))",
                w.kind.table()
            ),
            params![
                w.name,
                w.at,
                w.tx,
                result.digest,
                result.error,
                w.turtle,
                MAX_TURTLE_BYTES
            ],
        )?;
        Ok(updated != 0)
    }

    /// Report actual persisted progress, never treating failure as completion.
    pub fn registry_digest_backfill_status(&self) -> Result<RegistryDigestBackfillStatus> {
        let mut status = RegistryDigestBackfillStatus::default();
        for table in ["shapes", "ontologies"] {
            let counts: (i64, i64, i64) = self.conn.query_row(
                &format!(
                    "SELECT COALESCE(SUM(digest IS NULL AND digest_backfill_error IS NULL),0), \
                 COALESCE(SUM(digest IS NOT NULL),0), \
                 COALESCE(SUM(digest IS NULL AND digest_backfill_error IS NOT NULL),0) FROM {table}"
                ),
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            status.pending += counts.0;
            status.complete += counts.1;
            status.failed += counts.2;
        }
        Ok(status)
    }
}
