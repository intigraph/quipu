//! The preconditions of a reverse (DESIGN v2 step 0): the catalog pair
//! `R -> R+1` routes only this graph back to legacy, and the owner file is
//! held exclusively for the whole run, so no owner commit can land.

use super::{GraphFact, ReversePlan, refused};
use crate::graph_catalog::Catalog;
use crate::store::Store;
use crate::{Error, Result};

/// Refuse unless `next` is revision `R+1` of `current`, `current` routes
/// `graph` to its owner at generation `G`, and `next` differs only by routing
/// `graph` back to legacy (no route). The catalog file format has no
/// generation for a legacy-served graph, so `G+1` is pinned in the legacy
/// fence journal and the receipt.
pub fn validate_reverse_catalogs(
    current: &Catalog,
    next: &Catalog,
    plan: &ReversePlan<'_>,
) -> Result<()> {
    let (next_revision, _) = plan.next()?;
    if current.revision() != plan.catalog_revision || next.revision() != next_revision {
        return Err(refused(
            "catalogs must be revisions R and R+1 of this reverse",
        ));
    }
    if !current
        .routes()
        .any(|r| r.graph == plan.graph && r.generation == plan.generation)
    {
        return Err(refused(
            "catalog R does not route the graph at generation G",
        ));
    }
    if next.routes().any(|r| r.graph == plan.graph) {
        return Err(refuse_next());
    }
    let others = |c: &Catalog| {
        c.routes()
            .filter(|r| r.graph != plan.graph)
            .cloned()
            .collect::<Vec<_>>()
    };
    if others(current) != others(next)
        || current.legacy_store_id() != next.legacy_store_id()
        || current.legacy_participant() != next.legacy_participant()
    {
        return Err(refused("catalog R+1 changes something besides this graph"));
    }
    Ok(())
}

fn refuse_next() -> Error {
    refused("catalog R+1 must route the graph back to legacy (no owner route)")
}

/// Exclusive hold of the owner file for the whole reverse (DESIGN v2 step 0).
///
/// Two locks, both taken without waiting, so any other holder refuses:
///
/// - On Unix, an EXCLUSIVE flock on the owner's native writer lease
///   (`<owner>.writer-lease`). Every writable quipu `Store` holds a shared
///   lease for its whole life, so a running owner refuses even while idle.
/// - SQLite `locking_mode=EXCLUSIVE` set before the first access, then
///   `BEGIN EXCLUSIVE`. In WAL mode every other connection that has read the
///   file keeps a SHARED lock on it while open, so another process holding
///   the owner file open (idle, no transaction, any client) refuses.
///
/// The connection is `query_only` and never checkpoints on close, so holding
/// and dropping the lock writes nothing to the owner file. Read the owner's
/// graph through [`OwnerLock::export_routed_graph`]: a second connection to
/// the file is refused while the lock is held.
pub struct OwnerLock {
    store: Store,
    #[cfg(unix)]
    _lease: std::fs::File,
}

impl Drop for OwnerLock {
    fn drop(&mut self) {
        let _ = self.store.conn.execute_batch("ROLLBACK");
    }
}

impl OwnerLock {
    /// Every current fact of the owner's named `graph`, read under the lock.
    pub fn export_routed_graph(&self, graph: &str) -> Result<Vec<GraphFact>> {
        self.store.export_routed_graph(graph)
    }
}

fn in_use() -> Error {
    refused(
        "the owner file is in use by another process or connection; stop its owner before a reverse",
    )
}

#[cfg(unix)]
fn lock_owner_lease(path: &str) -> Result<std::fs::File> {
    use crate::store::ownership_latch::{no_follow, regular, sibling};
    let lease = sibling(std::path::Path::new(path), ".writer-lease")?;
    let file = no_follow(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false),
    )
    .open(&lease)
    .map_err(|e| refused(format!("owner writer lease: {e}")))?;
    regular(&file)?;
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .map_err(|_| in_use())?;
    Ok(file)
}

/// Take the exclusive hold of the owner file without waiting. Any other
/// process or connection holding the file open refuses, idle or not.
pub fn lock_owner_file(path: &str) -> Result<OwnerLock> {
    if !std::path::Path::new(path).is_file() {
        return Err(refused("the owner file does not exist"));
    }
    #[cfg(unix)]
    let lease = lock_owner_lease(path)?;
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        true,
    )?;
    conn.busy_timeout(std::time::Duration::ZERO)?;
    // locking_mode must precede the first access to the file.
    conn.execute_batch("PRAGMA locking_mode=EXCLUSIVE;")?;
    conn.execute_batch("BEGIN EXCLUSIVE")
        .map_err(|_| in_use())?;
    // Only once the lock is held: query_only also refuses BEGIN EXCLUSIVE.
    conn.execute_batch("PRAGMA query_only=ON;")?;
    Ok(OwnerLock {
        store: Store::with_connection(conn),
        #[cfg(unix)]
        _lease: lease,
    })
}
