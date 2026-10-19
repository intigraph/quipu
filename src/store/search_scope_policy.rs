//! An explicit, additive search-default migration; no fact or identity changes.
use rusqlite::Connection;

use super::Store;
use crate::{Error, Result};

fn has_table(conn: &Connection, table: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )?)
}

fn read_policy(conn: &Connection) -> Result<(&'static str, &'static str)> {
    if !has_table(conn, "search_default_policy")? {
        if has_table(conn, "facts")? {
            // A read-only/immutable legacy store is recognized without DDL.
            return Ok(("root", "legacy_unmarked_store"));
        }
        return Err(Error::Store("search default policy is unavailable".into()));
    }
    let mut statement = conn.prepare("SELECT id,policy FROM main.search_default_policy LIMIT 2")?;
    let mut rows = statement.query([])?;
    let Some(row) = rows.next()? else {
        return Err(Error::Store("missing search default policy".into()));
    };
    let id: i64 = row.get(0)?;
    let policy: String = row.get(1)?;
    if id != 1 || rows.next()?.is_some() {
        return Err(Error::Store(
            "invalid search default policy cardinality".into(),
        ));
    }
    match policy.as_str() {
        "all" => Ok(("all", "store_default")),
        "legacy-root" => Ok(("root", "store_legacy_compatibility")),
        _ => Err(Error::Store("invalid search default policy".into())),
    }
}

/// Schema creation and the first default choice share the same writer lock.
/// Concurrent opens cannot mistake newly-created facts for an older store.
pub(super) fn initialize_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result: Result<()> = (|| {
        let existing = has_table(conn, "facts")?;
        conn.execute_batch(crate::schema::INIT_SQL)?;
        if !has_table(conn, "search_default_policy")? {
            conn.execute_batch(
                "CREATE TABLE search_default_policy (
                    id INTEGER PRIMARY KEY CHECK(id=1),
                    policy TEXT NOT NULL CHECK(policy IN ('all','legacy-root'))
                )",
            )?;
            conn.execute(
                "INSERT INTO main.search_default_policy(id,policy) VALUES(1,?1)",
                [if existing { "legacy-root" } else { "all" }],
            )?;
        }
        read_policy(conn)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = conn.execute_batch("ROLLBACK");
        return Err(error);
    }
    if let Err(error) = conn.execute_batch("COMMIT") {
        let _ = conn.execute_batch("ROLLBACK");
        return Err(error.into());
    }
    Ok(())
}

impl Store {
    /// Resolve intent, never infer it from a UUID, FTS layout, or config age.
    pub(crate) fn default_search_graph_scope(&self) -> Result<(&'static str, &'static str)> {
        match self.search_config().default_graph_scope.as_deref() {
            Some("all") => Ok(("all", "configuration")),
            Some("root") => Ok(("root", "configuration")),
            Some(_) => Err(Error::InvalidValue(
                "search.default_graph_scope must be all or root".into(),
            )),
            None => read_policy(&self.conn),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn concurrent_initializers_make_one_fresh_choice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("concurrent.db");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let conn = Connection::open(path).unwrap();
                    conn.busy_timeout(std::time::Duration::from_secs(5))
                        .unwrap();
                    barrier.wait();
                    initialize_schema(&conn).unwrap();
                    assert_eq!(read_policy(&conn).unwrap(), ("all", "store_default"));
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    }

    #[test]
    fn failed_initialization_rolls_back_schema_and_policy_together() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE facts (unsupported INTEGER)")
            .unwrap();
        assert!(initialize_schema(&conn).is_err());
        assert!(conn.is_autocommit());
        assert!(!has_table(&conn, "search_default_policy").unwrap());
        assert!(!has_table(&conn, "transactions").unwrap());
        assert!(has_table(&conn, "facts").unwrap());
    }
}
