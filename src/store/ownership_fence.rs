//! Commit-time ownership fence for an already-open writable handle.
//!
//! Both callback installers share one admission pin and reader/check latch.
//! The reader ends its SQLite snapshot after every query; it never runs SQL
//! on the committing connection. Contention and poisoned/unavailable state
//! refuse without waiting while SQLite holds its writer lock.
//!
//! This is the unenrolled boundary only. No callback may promote an enrolled
//! lineage: observing enrollment permanently revokes this handle's legacy pin,
//! including after a local database rollback or callback replacement. There is
//! no authenticated startup adapter or protected-journal enrollment API yet.
//!
//! The check latch is NOT a retirement latch held through physical commit.
//! Admitting enrolled writes requires the reviewed per-lineage shared/exclusive
//! scope and protected-journal adapter; this increment admits none. SQLite's
//! pre-commit callback cannot by itself prove that a latch spans COMMIT.
//!
//! Generation pin (P1, aegis-s24.8): a handle may carry the protected-journal
//! path and the exact authority tuple it was admitted under. Every check then
//! also re-reads that journal (a file read, never SQL on the committing
//! connection) and refuses unless it is still ACTIVE at that tuple. A refusal
//! revokes the pin for good, so restoring older journal bytes cannot revive
//! the handle. No public path sets a pin yet: enrolled startup still refuses.
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use crate::error::Result;
use crate::ownership_handoff_model::Authority;

use rusqlite::{Connection, OpenFlags};

pub(super) struct CommitFence {
    reader: Mutex<Option<Connection>>,
    revoked: AtomicBool,
    journal: OnceLock<(PathBuf, Authority)>,
    // The first journal refusal, kept because a refused COMMIT surfaces only
    // as SQLite's generic constraint error.
    reason: OnceLock<String>,
}

impl CommitFence {
    /// In-memory/browser stores still pass the enrolled startup gate, but
    /// have no native placement or independent disk reader.
    pub(super) fn new(conn: &Connection) -> Option<Arc<Self>> {
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        let path = conn.path().filter(|p| !p.is_empty())?.to_owned();
        // Warm the WAL reader now: an INACTIVE legacy handle can retain its
        // original database after unlink. Each later query is still a fresh
        // read transaction, so adoption/enrollment cannot hide in a snapshot.
        let reader = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .and_then(|c| {
            c.execute_batch("PRAGMA query_only=ON;")?;
            c.query_row("SELECT count(*) FROM main.sqlite_master", [], |r| {
                r.get::<_, i64>(0)
            })?;
            Ok(c)
        })
        .ok();
        Some(Arc::new(Self {
            reader: Mutex::new(reader),
            revoked: AtomicBool::new(false),
            journal: OnceLock::new(),
            reason: OnceLock::new(),
        }))
    }

    pub(super) fn revoked(&self) -> bool {
        self.revoked.load(Ordering::Acquire)
    }

    /// An entrypoint may see enrollment and refuse before reaching COMMIT.
    /// That observation must revoke the same pin the callbacks retain.
    pub(super) fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
    }

    /// The generation pin: refuse, and revoke for good, unless the pinned
    /// journal still admits the pinned authority. Unpinned handles pass.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn check_journal(&self) -> Result<()> {
        let Some((path, authority)) = self.journal.get() else {
            return Ok(());
        };
        // Never wait here: the commit hook runs under SQLite's writer lock.
        // Contention means retirement holds the latch; refuse this commit but
        // do not revoke, since retirement may still abort. The journal decides.
        let _latch = super::ownership_latch::try_shared(&super::ownership_latch::path_for(path))?;
        super::ownership_journal::admits(path, authority).inspect_err(|e| {
            let _ = self.reason.set(e.to_string());
            self.revoke();
        })
    }

    /// Why the generation pin revoked this handle, if it did.
    pub(super) fn reason(&self) -> Option<&str> {
        self.reason.get().map(String::as_str)
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn check_journal(&self) -> Result<()> {
        Ok(())
    }

    /// Test fixtures only: there is no enrollment or admission path that
    /// could set a pin in production yet.
    #[cfg(test)]
    pub(super) fn pin_journal(&self, path: PathBuf, authority: Authority) {
        assert!(
            self.journal.set((path, authority)).is_ok(),
            "a handle is pinned once"
        );
    }

    pub(super) fn refuses(&self) -> bool {
        // Never block from a commit hook: the writer lock is already held.
        let Ok(reader) = self.reader.try_lock() else {
            return true;
        };
        // Do not carry a pre-latch false read across another check's
        // revocation. Entrypoints can also revoke without this reader latch;
        // recheck the monotonic pin after the SQL observation below.
        if self.revoked() {
            return true;
        }
        let Some(reader) = reader.as_ref() else {
            return true;
        };
        super::ownership::state_on(reader, Some(self)).is_err() || self.revoked()
    }

    #[cfg(test)]
    pub(super) fn with_check_latch(&self, f: impl FnOnce()) {
        let _guard = self.reader.lock().unwrap();
        f();
    }
}
