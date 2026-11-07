//! WAL checkpointing and size reporting (aegis-raq1ok).
//!
//! The deployed store reached a **1.06 GB** write-ahead log against a 5.7 GB
//! database, stable across samples and unchanged by a process restart, while
//! `wal_autocheckpoint` sat at SQLite's default of 1000 pages. Writes append to
//! the WAL and complete normally; reads then work against a WAL that cannot be
//! retired, and stall for 9-21 s at a time with work pending — long enough for
//! a 10 s liveness probe to read the store as dead and restart it.
//!
//! The mechanism to keep in mind when changing anything here: **SQLite cannot
//! checkpoint past the oldest live reader.** The server holds a pool of
//! long-lived read connections, so a checkpoint can be attempted, blocked, and
//! leave the WAL exactly where it was — which is what "checkpoints are running"
//! looked like from the outside while the WAL grew 250x past its threshold.
//! That is why `wal_bytes` is a metric and not an assumption.

use rusqlite::Connection;

use crate::Result;

/// Checkpoint-only connection: never carries the HTTP writer mutex and never
/// initializes or migrates a store. The serving handle's admission pin is shared,
/// so a later ownership refusal cannot be escaped by this maintenance handle.
pub struct PassiveCheckpointer {
    conn: Connection,
    fence: Option<std::sync::Arc<super::ownership_fence::CommitFence>>,
    identity: String,
}

impl PassiveCheckpointer {
    /// Move WAL pages without holding the serving writer mutex. PASSIVE avoids
    /// waiting for SQLite readers, but copying/fsync can still take seconds.
    pub fn checkpoint(&self) -> Result<CheckpointOutcome> {
        // Check on this connection, not through the commit-fence reader latch:
        // borrowing that latch could make a concurrent HTTP commit refuse.
        super::ownership::state_on(&self.conn, self.fence.as_deref())?;
        let identity: String = self.conn.query_row(
            "SELECT store_id FROM store_identity WHERE id=1",
            [],
            |row| row.get(0),
        )?;
        if identity != self.identity {
            return Err(crate::Error::InvalidValue(
                "WAL maintenance store identity changed".into(),
            ));
        }
        checkpoint_passive(&self.conn)
    }
}

impl super::Store {
    /// Open maintenance on the already-admitted disk store, without CREATE,
    /// schema setup, or a second writable Store. Anonymous stores have no WAL.
    pub fn passive_checkpointer(&self) -> Result<Option<PassiveCheckpointer>> {
        self.ensure_ownership_writable()?;
        if self.conn.is_readonly(rusqlite::MAIN_DB)? {
            return Err(crate::Error::PolicyDenied(
                "WAL maintenance requires the admitted writable handle".into(),
            ));
        }
        let Some(path) = self.conn.path().filter(|path| !path.is_empty()) else {
            return Ok(None);
        };
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.busy_timeout(std::time::Duration::ZERO)?;
        let checkpointer = PassiveCheckpointer {
            conn,
            fence: self.ownership_fence.clone(),
            identity: self.store_id()?,
        };
        // Validate identity and ownership before handing off. Do not checkpoint
        // here: construction runs before the writer mutex can be released.
        super::ownership_startup::require_unenrolled(&checkpointer.conn)?;
        super::ownership::state_on(&checkpointer.conn, checkpointer.fence.as_deref())?;
        let identity: String = checkpointer.conn.query_row(
            "SELECT store_id FROM store_identity WHERE id=1",
            [],
            |row| row.get(0),
        )?;
        if identity != checkpointer.identity {
            return Err(crate::Error::InvalidValue(
                "WAL maintenance store identity differs".into(),
            ));
        }
        Ok(Some(checkpointer))
    }
}

/// Outcome of a PASSIVE checkpoint: what SQLite actually managed to do.
///
/// Reported rather than discarded because "the checkpoint ran" and "the
/// checkpoint retired the log" are different facts, and this incident is
/// entirely about the gap between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointOutcome {
    /// Pages currently in the WAL after the attempt.
    pub wal_pages: i64,
    /// Pages successfully moved into the database file.
    pub moved_pages: i64,
    /// True when SQLite reported it was blocked (busy) rather than complete.
    pub blocked: bool,
}

impl CheckpointOutcome {
    /// Did this attempt actually retire the whole log?
    #[must_use]
    pub fn fully_retired(&self) -> bool {
        !self.blocked && self.wal_pages == self.moved_pages
    }
}

/// Run a **PASSIVE** checkpoint: move what can be moved, never block.
///
/// PASSIVE is the only safe mode to call on a live serving store. `FULL`,
/// `RESTART` and `TRUNCATE` wait for readers, and waiting for readers on this
/// server is precisely the stall being fixed — a blocking checkpoint here would
/// convert an intermittent read stall into a guaranteed one.
pub(crate) fn checkpoint_passive(conn: &Connection) -> Result<CheckpointOutcome> {
    // wal_checkpoint returns (busy, wal_pages, moved_pages).
    let (busy, wal_pages, moved_pages): (i64, i64, i64) =
        conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    Ok(CheckpointOutcome {
        wal_pages,
        moved_pages,
        blocked: busy != 0,
    })
}

/// Run a **TRUNCATE** checkpoint: retire the log AND reset the file to zero.
///
/// This BLOCKS on readers, so it is safe only where no reader can exist —
/// in practice at startup, before the read pool is created. That restriction is
/// the whole point: dearing's manual repair took the deployed WAL from
/// 1,062,898,232 bytes to 78,312 in 0.0 s *with the service stopped*, and the
/// return values said `log_frames=0 checkpointed=0` — the content had already
/// been checkpointed, and only the file reset had never happened, because
/// SQLite cannot reset the WAL past the oldest live reader and the pool holds
/// four of them for the process's whole life.
///
/// So the growth was stale FILE SPACE, not unwritten data. PASSIVE can never
/// fix it; only this can, and only before the readers exist.
pub(crate) fn checkpoint_truncate(conn: &Connection) -> Result<CheckpointOutcome> {
    let (busy, wal_pages, moved_pages): (i64, i64, i64) =
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    Ok(CheckpointOutcome {
        wal_pages,
        moved_pages,
        blocked: busy != 0,
    })
}

#[cfg(test)]
mod tests {
    use crate::Store;

    #[test]
    fn independent_checkpoint_completes_while_serving_writer_is_held() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("maintenance.db");
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        crate::tool_knot(
            &mut store,
            &serde_json::json!({
                "turtle": "<urn:checkpoint:entity> <urn:checkpoint:value> 1 ."
            }),
        )
        .unwrap();
        assert!(store.wal_bytes().unwrap() > 0);
        let checkpointer = store.passive_checkpointer().unwrap().unwrap();
        let writer = std::sync::Mutex::new(store);
        let held = writer.lock().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            send.send(checkpointer.checkpoint()).unwrap();
        });
        let result = receive.recv_timeout(std::time::Duration::from_secs(2));
        drop(held);
        worker.join().unwrap();
        assert!(result.unwrap().unwrap().fully_retired());
        let store = writer.lock().unwrap();
        let entity = store.lookup("urn:checkpoint:entity").unwrap().unwrap();
        let attribute = store.lookup("urn:checkpoint:value").unwrap().unwrap();
        assert!(store.current_facts().unwrap().iter().any(|fact| {
            fact.entity == entity
                && fact.attribute == attribute
                && fact.value == crate::Value::Int(1)
        }));
    }

    #[test]
    fn independent_checkpoint_preserves_enrollment_and_identity_refusals() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("maintenance.db");
        let store = Store::open(path.to_str().unwrap()).unwrap();
        let reader = Store::open_read_only(path.to_str().unwrap()).unwrap();
        assert!(reader.passive_checkpointer().is_err());
        let checkpointer = store.passive_checkpointer().unwrap().unwrap();
        store
            .conn
            .execute_batch("CREATE TABLE ownership_handoff_enrollment(value);")
            .unwrap();
        assert!(checkpointer.checkpoint().is_err());
        assert!(store.passive_checkpointer().is_err());
        // Simulate an external rollback of the enrollment marker. The normal
        // serving connection correctly refuses that DROP after revocation.
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch("DROP TABLE ownership_handoff_enrollment;")
            .unwrap();
        assert!(
            checkpointer.checkpoint().is_err(),
            "revocation must survive marker rollback"
        );
        assert!(store.ensure_ownership_writable().is_err());
        assert!(
            Store::open_in_memory()
                .unwrap()
                .passive_checkpointer()
                .unwrap()
                .is_none()
        );

        let other_path = dir.path().join("identity.db");
        let other = Store::open(other_path.to_str().unwrap()).unwrap();
        let checkpointer = other.passive_checkpointer().unwrap().unwrap();
        other
            .conn
            .execute(
                "UPDATE store_identity SET store_id='changed' WHERE id=1",
                [],
            )
            .unwrap();
        assert!(checkpointer.checkpoint().is_err());
    }

    /// A file-backed store reports a WAL size and a PASSIVE checkpoint retires
    /// the log when nothing is reading it.
    ///
    /// The anti-vacuity arm matters here: if the WAL were empty the "retired"
    /// assertion would hold for free, so the test WRITES first and asserts the
    /// log was non-empty before checkpointing it.
    #[test]
    fn passive_checkpoint_retires_the_log_when_no_reader_holds_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let path = path.to_str().unwrap();
        let mut store = Store::open(path).unwrap();
        for i in 0..200 {
            let e = store.intern(&format!("http://ex/e{i}")).unwrap();
            let a = store.intern("http://ex/p").unwrap();
            let v = store.intern(&format!("http://ex/v{i}")).unwrap();
            store
                .transact(
                    &[crate::store::Datum {
                        entity: e,
                        attribute: a,
                        value: crate::types::Value::Ref(v),
                        valid_from: "2026-09-06T00:00:00Z".into(),
                        valid_to: None,
                        op: crate::types::Op::Assert,
                    }],
                    "2026-09-06T00:00:00Z",
                    None,
                    None,
                )
                .unwrap();
        }
        // ANTI-VACUITY: there must be something to retire.
        let before = store
            .wal_bytes()
            .expect("file-backed store reports a WAL size");
        assert!(
            before > 0,
            "fixture broken: the WAL is empty, so 'the checkpoint retired it' is free"
        );

        let out = store.checkpoint_wal_passive().unwrap();
        assert!(
            out.fully_retired(),
            "PASSIVE checkpoint did not retire the log with no reader holding it: \
             {} of {} pages moved, blocked={}",
            out.moved_pages,
            out.wal_pages,
            out.blocked
        );
    }

    /// `wal_bytes` reports UNKNOWN for an in-memory store rather than 0.
    ///
    /// This is the guard against the exact shape of the incident: a gauge that
    /// answers "0" when it cannot see the WAL lets a blind instrument assert
    /// health. Absence must stay distinguishable from emptiness.
    #[test]
    fn wal_bytes_is_unknown_not_zero_for_a_store_with_no_wal_on_disk() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(
            store.wal_bytes(),
            None,
            "an in-memory store reported a WAL SIZE; 0 here would read as 'the log is empty' \
             when the truth is 'there is no log to look at' (aegis-raq1ok)"
        );
    }
}
