//! Pooled read-only composition connections for the Phase 2 composed reader
//! (aegis-n0wc.5, seam A; dearing conditions P1-P3, n0wc.5 c32).
//!
//! One connection opens the legacy file `SQLITE_OPEN_READ_ONLY` as `main`.
//! Each read attaches exactly the routed files it selected, with `mode=ro`.
//! No file is written: SQLite refuses writes to `main` and to every
//! attachment. Only the per-connection TEMP schema holds derived state, the
//! term alias map, and it is rebuilt inside the read's own snapshot.
//!
//! - **P1 clean checkout.** Returning a connection rolls back its transaction,
//!   drops every TEMP object and detaches every schema. Checkout VERIFIES that:
//!   `PRAGMA database_list` names only `main` (plus an empty `temp`), and the
//!   TEMP schema holds no object. A connection that fails the check is
//!   discarded, never reused.
//! - **P2 no cross-query state.** A checkout wraps the connection in a FRESH
//!   [`Store`], so no term, read-model or projection cache outlives one read.
//! - **P3 one pin per checkout.** The snapshot begins in [`Checkout::begin`],
//!   which the dataset calls while every participant's commit fence is held,
//!   and it ends when the checkout returns.

// Dormant like `GraphSetOwner::open_enrolled`: no production caller until the
// Phase 2 reader is wired to a route (aegis-n0wc.5).
#![cfg_attr(not(test), allow(dead_code))]

use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags};

use std::collections::BTreeMap;

use super::Store;
use super::attach::{self, Attachment, FACTS_COLUMNS};
use super::graph_owner::CapturedRead;
use crate::error::{Error, Result};
use crate::read_authorization::mutants::{Mutant, active};
use crate::schema::SPACE_SIZE;

fn refused(reason: &str) -> Error {
    Error::PolicyDenied(format!("graph-set composition refused: {reason}"))
}

/// Named, non-authorization refusal. The selection is already authorized
/// when this runs, so naming the reason discloses no ungranted graph.
fn space_refused(reason: &str) -> Error {
    Error::Store(format!("composed mount refused: term space {reason}"))
}

/// Prove the participant files occupy pairwise-disjoint term spaces.
pub(crate) fn check_term_spaces(reads: &[CapturedRead<'_>]) -> Result<()> {
    // One physical file can carry several selected graphs.
    let mut files: BTreeMap<&str, &CapturedRead<'_>> = BTreeMap::new();
    for read in reads {
        files.entry(read.route.store_id.as_str()).or_insert(read);
    }
    let mut spaces: BTreeMap<i64, &str> = BTreeMap::new();
    for (store_id, read) in files {
        let space = read.store.local_term_space()?;
        if space == 0 && !read.legacy {
            return Err(space_refused(
                "0 is reserved for the legacy file; enroll the set file into its own term space",
            ));
        }
        if let Some(other) = spaces.insert(space, store_id) {
            return Err(space_refused(&format!(
                "{space} is shared by two participant files ({other}, {store_id})"
            )));
        }
        if active(Mutant::AttachTrustDeclaredSpace) {
            continue;
        }
        let (low, high): (Option<i64>, Option<i64>) =
            read.store
                .conn
                .query_row("SELECT min(id), max(id) FROM terms", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
        let floor = space
            .checked_mul(SPACE_SIZE)
            .ok_or_else(|| space_refused("out of range"))?;
        let ceiling = floor
            .checked_add(SPACE_SIZE)
            .ok_or_else(|| space_refused("out of range"))?;
        if let (Some(low), Some(high)) = (low, high)
            && (low < floor.max(1) || high >= ceiling)
        {
            return Err(space_refused(&format!(
                "{space} is declared but the file holds ids outside it"
            )));
        }
    }
    Ok(())
}

/// SQLite's default compiled `SQLITE_MAX_ATTACHED` (the bundled build does not
/// raise it).
const SQLITE_MAX_ATTACHED: usize = 10;

/// A bounded pool of read-only composition connections over one legacy file.
pub(crate) struct CompositionPool {
    legacy_path: String,
    capacity: usize,
    /// Bounded attached set: the first composed reader refuses a selection
    /// needing more attachments BEFORE any ATTACH (design: attachment limit).
    max_attached: usize,
    state: Mutex<PoolState>,
}

struct PoolState {
    idle: Vec<Connection>,
    open: usize,
}

impl CompositionPool {
    pub(crate) fn new(
        legacy_path: &str,
        capacity: usize,
        max_attached: usize,
    ) -> Result<Arc<Self>> {
        if capacity == 0 {
            return Err(refused("composition pool needs at least one connection"));
        }
        Ok(Arc::new(Self {
            legacy_path: legacy_path.to_owned(),
            capacity,
            // Never above SQLite's default compiled SQLITE_MAX_ATTACHED: past it
            // ATTACH itself fails mid-mount instead of refusing up front.
            max_attached: max_attached.min(SQLITE_MAX_ATTACHED),
            state: Mutex::new(PoolState {
                idle: Vec::new(),
                open: 0,
            }),
        }))
    }

    fn open_connection(&self) -> Result<Connection> {
        let conn = Connection::open_with_flags(
            &self.legacy_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;
        // No `query_only`: it would also refuse the TEMP alias map. The
        // read-only open flag and `mode=ro` attachments protect every file.
        conn.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA mmap_size=268435456; PRAGMA cache_size=-32000;",
        )?;
        Ok(conn)
    }

    /// Check out a verified-clean connection. Never waits: an exhausted pool
    /// refuses, like every other strict participant.
    pub(crate) fn checkout(self: &Arc<Self>) -> Result<Checkout> {
        let reused = {
            let mut state = self.state.lock();
            match state.idle.pop() {
                Some(conn) => Some(conn),
                None if state.open < self.capacity => {
                    state.open += 1;
                    None
                }
                None => return Err(refused("composition connection pool occupied")),
            }
        };
        let conn = match reused {
            Some(conn) if active(Mutant::CompositionSkipCheckoutVerify) || is_clean(&conn)? => conn,
            // P1: a dirty connection is discarded and replaced, never reused.
            Some(_dirty) => self.open_connection().inspect_err(|_| self.forget())?,
            None => self.open_connection().inspect_err(|_| self.forget())?,
        };
        if !active(Mutant::CompositionSkipCheckoutVerify) && !is_clean(&conn)? {
            self.forget();
            return Err(refused("a fresh composition connection is not clean"));
        }
        Ok(Checkout {
            max_attached: self.max_attached,
            pool: self.clone(),
            store: Some(Store::with_connection(conn)),
            aliases: Vec::new(),
            files: Vec::new(),
        })
    }

    fn forget(&self) {
        let mut state = self.state.lock();
        state.open = state.open.saturating_sub(1);
    }

    /// Cleanup already ran; checkout is where cleanliness is VERIFIED (P1).
    fn give_back(&self, conn: Connection) {
        self.state.lock().idle.push(conn);
    }

    #[cfg(test)]
    pub(crate) fn legacy_path_for_test(&self) -> String {
        self.legacy_path.clone()
    }

    #[cfg(test)]
    pub(crate) fn idle_len(&self) -> usize {
        self.state.lock().idle.len()
    }
}

/// P1 predicate: only `main` mounted (an empty `temp` is allowed), no TEMP
/// object, and no open transaction.
pub(crate) fn is_clean(conn: &Connection) -> Result<bool> {
    if !conn.is_autocommit() {
        return Ok(false);
    }
    let mut stmt = conn.prepare("SELECT name FROM pragma_database_list")?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if names.iter().any(|n| n != "main" && n != "temp") {
        return Ok(false);
    }
    let temp: i64 = conn.query_row("SELECT count(*) FROM temp.sqlite_master", [], |r| r.get(0))?;
    Ok(temp == 0)
}

/// One routed file mounted for one read.
pub(crate) struct Mount {
    pub store_id: String,
    pub alias: String,
    pub path: String,
}

/// A checked-out composition connection. Dropping it returns the connection
/// through the P1 cleanup; a connection that cannot be cleaned is discarded.
pub(crate) struct Checkout {
    max_attached: usize,
    pool: Arc<CompositionPool>,
    store: Option<Store>,
    aliases: Vec<String>,
    files: Vec<(String, String)>,
}

impl Checkout {
    /// The schema a participant file is mounted as: `main` for the legacy
    /// file, otherwise its attachment alias.
    pub(crate) fn schema_for(&self, store_id: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|(id, _)| id == store_id)
            .map(|(_, alias)| alias.as_str())
    }

    fn store(&self) -> &Store {
        self.store
            .as_ref()
            .expect("checkout holds its store until drop")
    }

    /// ATTACH the selected routed files and begin ONE read snapshot across
    /// `main` and every attachment. The caller holds every participant's
    /// commit fence, so this snapshot is the strict cut (P3).
    pub(crate) fn begin(&mut self, mounts: &[Mount]) -> Result<()> {
        if !self.aliases.is_empty() || !self.store().conn.is_autocommit() {
            return Err(refused("checkout already began a snapshot"));
        }
        if mounts.len() > self.max_attached {
            return Err(refused(&format!(
                "selection needs {} attached files, over the attachment limit of {}",
                mounts.len(),
                self.max_attached
            )));
        }
        let attachments: Vec<Attachment> = mounts
            .iter()
            .map(|m| Attachment::read_only(&m.alias, &m.path))
            .collect();
        // Record aliases first so a failed ATTACH is still detached on drop.
        self.aliases = mounts.iter().map(|m| m.alias.clone()).collect();
        self.files = mounts
            .iter()
            .map(|m| (m.store_id.clone(), m.alias.clone()))
            .collect();
        let conn = &self.store().conn;
        attach::attach_all(conn, &attachments)?;
        let local = super::local_term_space(conn)?;
        attach::verify_attached_schema(conn, &attachments, local)?;
        conn.execute_batch("BEGIN DEFERRED")?;
        // Touching each schema starts its snapshot now, under the fences.
        let mut schemas = vec!["main".to_owned()];
        schemas.extend(self.aliases.iter().cloned());
        for schema in &schemas {
            conn.query_row(
                &format!("SELECT count(*) FROM {schema}.terms WHERE id = 0"),
                [],
                |_| Ok(()),
            )?;
        }
        Ok(())
    }

    /// Finish the composition inside the snapshot: pinned alias map, a facts
    /// source restricted to exactly the selected graph ids per schema, and the
    /// composed resolve SQL. Returns the store for one view.
    pub(crate) fn compose(
        &mut self,
        main_graphs: &[i64],
        attached_graphs: &[(String, Vec<i64>)],
    ) -> Result<()> {
        let attachments: Vec<Attachment> = self
            .aliases
            .iter()
            .map(|a| Attachment::read_only(a, ""))
            .collect();
        let store = self
            .store
            .as_mut()
            .expect("checkout holds its store until drop");
        super::alias::build_term_alias(&store.conn, &attachments)?;
        let cols = FACTS_COLUMNS.join(", ");
        let ids = |graphs: &[i64]| {
            if graphs.is_empty() {
                "NULL".to_owned()
            } else {
                graphs
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            }
        };
        let mut branches = vec![format!(
            "SELECT {cols} FROM main.facts WHERE g IN ({})",
            ids(main_graphs)
        )];
        let mut contributed = Vec::new();
        for (alias, graphs) in attached_graphs {
            if !self.aliases.contains(alias) {
                return Err(refused("composed graph names an unmounted file"));
            }
            branches.push(format!(
                "SELECT {cols} FROM {alias}.facts WHERE g IN ({})",
                ids(graphs)
            ));
            contributed.extend(graphs.iter().copied());
        }
        contributed.sort_unstable();
        // The graph registry, shadowed in TEMP (unqualified `graphs` reads
        // resolve to temp first): main's rows plus ONLY the selected attached
        // graphs, each marked with its source schema as `register_attached_graphs`
        // would, so no file is written and unselected graphs stay unregistered.
        let cols: Vec<String> = store
            .conn
            .prepare("SELECT name FROM pragma_table_info('graphs', 'main')")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        store
            .conn
            .execute_batch("CREATE TEMP TABLE graphs AS SELECT * FROM main.graphs;")?;
        for (alias, graphs) in attached_graphs {
            let present: Vec<String> = store
                .conn
                .prepare(&format!(
                    "SELECT name FROM pragma_table_info('graphs', '{alias}')"
                ))?
                .query_map([], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            let select: Vec<String> = cols
                .iter()
                .map(|c| {
                    if c == "source" {
                        format!("'{alias}'")
                    } else if present.contains(c) {
                        c.clone()
                    } else {
                        "NULL".to_owned()
                    }
                })
                .collect();
            store.conn.execute(
                &format!(
                    "INSERT OR REPLACE INTO temp.graphs ({}) SELECT {} FROM {alias}.graphs WHERE g IN ({})",
                    cols.join(","),
                    select.join(","),
                    ids(graphs)
                ),
                [],
            )?;
        }
        store.facts_source = format!("({}) AS facts", branches.join(" UNION ALL "));
        store.resolve_sql = attach::build_resolve_sql(&attachments);
        store.attachments = attachments;
        store.attached_graphs = contributed;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn is_clean_now(&self) -> Result<bool> {
        is_clean(&self.store().conn)
    }

    /// Hand the composed store to a view; it comes back through [`Self::restore`].
    pub(crate) fn take_store(&mut self) -> Store {
        self.store
            .take()
            .expect("checkout holds its store until drop")
    }

    pub(crate) fn restore(&mut self, store: Store) {
        self.store = Some(store);
    }
}

/// An empty in-memory store to swap into a view while its composed store
/// goes back to the pool. `None` if one cannot be opened: the checkout then
/// discards its connection instead of reusing it.
pub(crate) fn placeholder() -> Option<Store> {
    Connection::open_in_memory()
        .ok()
        .map(Store::with_connection)
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let Some(store) = self.store.take() else {
            // The store never came back: its connection is lost, not reused.
            self.pool.forget();
            return;
        };
        let conn = store.conn;
        let mut ok = true;
        if !conn.is_autocommit() {
            ok &= conn.execute_batch("ROLLBACK").is_ok();
        }
        let temp: Vec<(String, String)> = conn
            .prepare("SELECT type, name FROM temp.sqlite_master WHERE type IN ('table','view','trigger')")
            .and_then(|mut stmt| {
                stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<std::result::Result<Vec<_>, _>>()
            })
            .unwrap_or_else(|_| {
                ok = false;
                Vec::new()
            });
        for (kind, name) in temp {
            ok &= conn
                .execute_batch(&format!(
                    "DROP {} IF EXISTS temp.\"{}\"",
                    kind.to_uppercase(),
                    name
                ))
                .is_ok();
        }
        for alias in &self.aliases {
            if active(Mutant::CompositionSkipDetach) {
                continue;
            }
            // Aliases were validated by `attach_all`; a never-attached one errors harmlessly.
            let _ = conn.execute_batch(&format!("DETACH DATABASE {alias}"));
        }
        if ok {
            self.pool.give_back(conn);
        } else {
            drop(conn);
            self.pool.forget();
        }
    }
}
