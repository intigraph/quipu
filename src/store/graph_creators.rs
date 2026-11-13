//! Durable creator provenance, independent of the bounded delivery event log.
use super::Store;
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

impl Store {
    /// Backfill only locally owned registered graphs from retained provenance.
    /// Missing events mean unknown, never attribution to a later caller.
    /// The additive migration is atomic and is never run on read-only opens.
    pub(super) fn migrate_graph_creators_v1(conn: &Connection) -> Result<()> {
        conn.execute_batch("SAVEPOINT quipu_graph_creators_v1")?;
        let result = (|| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS graph_creators (\
                   g INTEGER PRIMARY KEY REFERENCES graphs(g) ON DELETE CASCADE,\
                   actor TEXT CHECK(actor IS NULL OR typeof(actor)='text'));",
            )?;
            require_registry(conn, "main")?;
            let mut query = conn.prepare(
                "SELECT g.g,t.iri FROM graphs g JOIN terms t ON t.id=g.g \
                 WHERE g.source IS NULL AND NOT EXISTS \
                 (SELECT 1 FROM graph_creators c WHERE c.g=g.g)",
            )?;
            let rows =
                query.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (g, graph) = row?;
                // Same strict validation as metadata reads, before any pruning.
                let actor = super::graph_metadata::event_creator(conn, "main", &graph)?;
                conn.execute(
                    "INSERT INTO graph_creators(g,actor) VALUES(?1,?2)",
                    params![g, actor.as_str()],
                )?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                super::release::release(conn, "quipu_graph_creators_v1")?;
                Ok(())
            }
            Err(error) => {
                let _ = super::release::rollback_release(conn, "quipu_graph_creators_v1");
                Err(error)
            }
        }
    }
}

/// Read from the selected source only. Old immutable packs have no registry;
/// their retained event provenance remains the compatibility read path.
pub(super) fn creator(conn: &Connection, schema: &str, graph: &str) -> Result<Option<Value>> {
    let exists: bool = conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {schema}.sqlite_master WHERE type='table' AND name='graph_creators')"),
        [], |r| r.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    require_registry(conn, schema)?;
    let actor: Option<Option<String>> = conn.query_row(
        &format!("SELECT c.actor FROM {schema}.graph_creators c JOIN {schema}.terms t ON t.id=c.g WHERE t.iri=?1"),
        [graph], |r| r.get(0),
    ).optional()?;
    Ok(actor.map(|actor| json!(actor)))
}

fn require_registry(conn: &Connection, schema: &str) -> Result<()> {
    let mut query = conn.prepare(&format!("PRAGMA {schema}.table_info(graph_creators)"))?;
    let columns = query
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if columns
        != [
            ("g".to_string(), "INTEGER".to_string(), 1),
            ("actor".to_string(), "TEXT".to_string(), 0),
        ]
    {
        return Err(Error::Store(
            "invalid durable graph creator registry".into(),
        ));
    }
    let mut query = conn.prepare(&format!("PRAGMA {schema}.foreign_key_list(graph_creators)"))?;
    let keys = query
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(6)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if keys
        != [(
            "graphs".to_string(),
            "g".to_string(),
            "g".to_string(),
            "CASCADE".to_string(),
        )]
    {
        return Err(Error::Store(
            "invalid durable graph creator registry ownership".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "graph_creators_tests.rs"]
mod tests;
