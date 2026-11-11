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
//! The check latch is NOT a retirement latch. The source-only explicit
//! `ownership_scope` holds a separate shared lineage latch across actual SQL
//! completion, with an exclusive journal-transition counterpart. A pinned
//! write outside that scope refuses. Enrollment/startup still admits none.
//!
//! Generation pin (P1, aegis-s24.8): a handle may carry the protected-journal
//! path and the exact authority tuple it was admitted under. Every check then
//! also re-reads that journal (a file read, never SQL on the committing
//! connection) and refuses unless it is still ACTIVE at that tuple. A refusal
//! revokes the pin for good, so restoring older journal bytes cannot revive
//! the handle. No public path sets a pin yet: enrolled startup still refuses.
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::AtomicU8;
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
    // 0 = no permit, 1 = private body (no outer commit), 2 = outer SQL seam.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) commit_scope: AtomicU8,
    #[cfg(test)]
    pub(super) before_commit: Mutex<Option<Box<dyn Fn() + Send>>>,
    #[cfg(test)]
    pub(super) after_sql: Mutex<Option<Box<dyn Fn() + Send>>>,
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
            #[cfg(not(target_arch = "wasm32"))]
            commit_scope: AtomicU8::new(0),
            #[cfg(test)]
            before_commit: Mutex::new(None),
            #[cfg(test)]
            after_sql: Mutex::new(None),
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
        super::ownership_journal::admits(path, authority).inspect_err(|e| {
            let _ = self.reason.set(e.to_string());
            self.revoke();
        })
    }

    /// Why the generation pin revoked this handle, if it did.
    pub(super) fn reason(&self) -> Option<&str> {
        self.reason.get().map(String::as_str)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn journal_path(&self) -> Option<&std::path::Path> {
        self.journal.get().map(|(path, _)| path.as_path())
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
        if super::ownership::state_on(reader, Some(self)).is_err() || self.revoked() {
            return true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let phase = self.commit_scope.load(Ordering::Acquire);
            if phase == 1 || (self.journal.get().is_some() && phase != 2) {
                return true;
            }
        }
        #[cfg(test)]
        if let Some(pause) = self.before_commit.lock().unwrap().as_ref() {
            pause();
        }
        false
    }

    #[cfg(test)]
    pub(super) fn with_check_latch(&self, f: impl FnOnce()) {
        let _guard = self.reader.lock().unwrap();
        f();
    }
}
