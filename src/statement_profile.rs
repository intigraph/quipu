//! Per-query SQLite statement totals (aegis-xacr), off unless a server enables it.
//!
//! A served query's `store_held_ms` says how long it held a connection, not
//! where that time went. On the production store a two-pattern star held a
//! reader for ~0.4 s that the same binary answers in ~0.01 s on a copy, and no
//! host-side measurement could attribute it to one query. This records, for one
//! query, how many statements SQLite finished, how many rows they produced,
//! how long SQLite spent in them, and which statement texts cost the most, so
//! the served log itself says whether the time is inside SQLite or not.
//!
//! SQLite's profile callback carries no connection or request identity, so the
//! totals are thread-local and count only while [`begin`] has armed the thread.
//! `sparql_structure::on_deep_stack` carries the armed state into its worker
//! thread and folds the worker's totals back, so a query evaluated there is
//! counted where it was armed.
//!
//! SQLite times each statement with the VFS clock, which on unix has
//! millisecond resolution, so one fast statement reads 0 or 1 ms. Summed over
//! many statements the error averages out; the statement and row counts are
//! exact. Statement texts are the engine's SQL with parameters bound, not the
//! caller's query.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use rusqlite::trace::{TraceEvent, TraceEventCodes};

/// Statement texts longer than this are truncated in the per-statement totals.
const SQL_PREFIX: usize = 160;

/// Totals for one armed span.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Totals {
    /// Statements SQLite finished.
    pub statements: u64,
    /// Result rows produced.
    pub rows: u64,
    /// Time SQLite reported inside those statements.
    pub nanos: u64,
    /// Per statement text (truncated): (finished count, nanoseconds).
    pub by_sql: HashMap<String, (u64, u64)>,
}

impl Totals {
    fn absorb(&mut self, other: Totals) {
        self.statements += other.statements;
        self.rows += other.rows;
        self.nanos += other.nanos;
        for (sql, (n, ns)) in other.by_sql {
            let entry = self.by_sql.entry(sql).or_default();
            entry.0 += n;
            entry.1 += ns;
        }
    }

    /// The `k` statement texts that cost the most time, most expensive first.
    #[must_use]
    pub fn top(&self, k: usize) -> Vec<(String, u64, u64)> {
        let mut all: Vec<(String, u64, u64)> = self
            .by_sql
            .iter()
            .map(|(sql, (n, ns))| (sql.clone(), *n, *ns))
            .collect();
        all.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        all.truncate(k);
        all
    }
}

thread_local! {
    static ACTIVE: RefCell<Option<Totals>> = const { RefCell::new(None) };
}

fn callback(event: TraceEvent<'_>) {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let Some(totals) = active.as_mut() else {
            return;
        };
        match event {
            TraceEvent::Profile(stmt, duration) => {
                let nanos = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
                totals.statements += 1;
                totals.nanos = totals.nanos.saturating_add(nanos);
                let sql = stmt.sql();
                let key: String = sql.chars().take(SQL_PREFIX).collect();
                let entry = totals.by_sql.entry(key).or_default();
                entry.0 += 1;
                entry.1 = entry.1.saturating_add(nanos);
            }
            TraceEvent::Row(_) => totals.rows += 1,
            _ => {}
        }
    });
}

/// Install the profile hook on `conn`. Costs nothing on threads not armed.
pub fn enable(conn: &rusqlite::Connection) {
    conn.trace_v2(
        TraceEventCodes::SQLITE_TRACE_PROFILE | TraceEventCodes::SQLITE_TRACE_ROW,
        Some(callback),
    );
}

/// Start counting on this thread, discarding anything unread.
pub fn begin() {
    ACTIVE.with(|a| *a.borrow_mut() = Some(Totals::default()));
}

/// Stop counting on this thread and return what was counted since [`begin`].
pub fn take() -> Option<Totals> {
    ACTIVE.with(|a| a.borrow_mut().take())
}

/// Whether this thread is counting.
#[must_use]
pub fn armed() -> bool {
    ACTIVE.with(|a| a.borrow().is_some())
}

/// Add a worker thread's totals to this thread's, if this thread is counting.
pub fn absorb(worker: Option<Totals>) {
    if let Some(worker) = worker {
        ACTIVE.with(|a| {
            if let Some(totals) = a.borrow_mut().as_mut() {
                totals.absorb(worker);
            }
        });
    }
}

/// `Duration` of the SQLite time in `totals`, for logging.
#[must_use]
pub fn sqlite_time(totals: &Totals) -> Duration {
    Duration::from_nanos(totals.nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        enable(&conn);
        conn
    }

    // A CTE, not a table: the share-completeness audit classifies every table
    // the source creates.
    const ROWS: &str = "WITH t(x) AS (VALUES (1),(2),(3)) SELECT x FROM t";

    fn rows(conn: &rusqlite::Connection) -> usize {
        let mut stmt = conn.prepare(ROWS).unwrap();
        stmt.query_map([], |r| r.get::<_, i64>(0)).unwrap().count()
    }

    #[test]
    fn counts_only_while_armed() {
        let conn = conn();
        assert_eq!(rows(&conn), 3);
        assert!(take().is_none(), "nothing armed, nothing counted");
        begin();
        assert_eq!(rows(&conn), 3);
        assert_eq!(rows(&conn), 3);
        let totals = take().unwrap();
        assert_eq!(totals.statements, 2);
        assert_eq!(totals.rows, 6);
        assert_eq!(totals.top(3)[0].0, ROWS);
        assert_eq!(totals.top(3)[0].1, 2);
        assert!(!armed());
    }

    #[test]
    fn a_worker_thread_folds_back_into_the_caller() {
        let conn = std::sync::Mutex::new(conn());
        begin();
        let worker = std::thread::scope(|s| {
            s.spawn(|| {
                begin();
                rows(&conn.lock().unwrap());
                take()
            })
            .join()
            .unwrap()
        });
        absorb(worker);
        rows(&conn.lock().unwrap());
        let totals = take().unwrap();
        assert_eq!(totals.statements, 2);
        assert_eq!(totals.rows, 6);
    }
}
