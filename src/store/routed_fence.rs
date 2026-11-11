//! Legacy-file fence for graphs moved to their own owner (per-graph pilot,
//! router step). Once a graph is fenced, every write to the legacy file that
//! would change that graph's state is refused by SQLite itself, whatever path
//! or process issues it: server, CLI, imports, forks, maintenance, events or a
//! direct transaction batch.
//!
//! The fence lives in the legacy file: a `routed_graph_fence` table naming the
//! routed graph IRIs, and BEFORE triggers on every graph-bearing column. The
//! columns are discovered from the live schema each time, never from a
//! hand-maintained list. Each discovered column must have a reviewed class
//! below; an unclassified graph-bearing column refuses installation and
//! verification, so a new table cannot silently become a bypass.
//!
//! The fence is installed only in the legacy file, never in an owner file.
//! Retained legacy copies stay byte-for-byte readable for reverse
//! reconciliation. Removing a fence row is the inverse; doing so after the
//! owner accepted writes still needs fenced reverse reconciliation first.

use super::Store;
use crate::{Error, Result};

/// How a graph-bearing column is fenced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceClass {
    /// Authoritative graph state: insert, update and delete are refused.
    /// Deleting would destroy the retained copy reverse reconciliation needs.
    All,
    /// Logs, staging and consumer cursors: new or changed rows are refused,
    /// deletes are allowed so retention and expiry keep running.
    WritesOnly,
    /// Names a graph without holding its data (dataset configuration that a
    /// composed-read consumer migration must be able to edit). Not fenced.
    Reference,
    /// Rebuilt from `facts`, which is fenced. Not fenced, so a rebuild of a
    /// retained copy reproduces the same rows instead of aborting.
    Derived,
}

/// The reviewed classification of every graph-bearing column in the schema.
const CLASSES: &[(&str, &str, FenceClass)] = &[
    ("facts", "g", FenceClass::All),
    ("graphs", "g", FenceClass::All),
    ("forks", "g", FenceClass::All),
    ("writer_graph_instances", "g", FenceClass::All),
    ("frozen_packs", "graph_iri", FenceClass::All),
    ("pendant_repo_graphs", "graph", FenceClass::All),
    ("graph_events", "graph", FenceClass::WritesOnly),
    ("graph_consumers", "graph", FenceClass::WritesOnly),
    ("snapshot_uploads", "graph", FenceClass::WritesOnly),
    ("denial_quarantine", "graph", FenceClass::WritesOnly),
    ("dataset_members", "graph_iri", FenceClass::Reference),
    ("substring_facts", "g", FenceClass::Derived),
];

/// Tables created on demand by a reviewed derivation (projections).
const DERIVED_PREFIXES: &[(&str, &str)] = &[("qp_proj_", "g")];

const FENCE_TABLE: &str = "routed_graph_fence";
const TRIGGER_PREFIX: &str = "quipu_routed_fence_";
const MESSAGE: &str = "graph owner: this graph is routed to its own owner; legacy write refused";

/// One discovered graph-bearing column and its class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FencedColumn {
    pub table: String,
    pub column: String,
    pub class: FenceClass,
}

fn refused(message: impl std::fmt::Display) -> Error {
    Error::PolicyDenied(format!("graph owner: {message}"))
}

fn identifier(name: &str) -> Result<&str> {
    if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        Ok(name)
    } else {
        Err(refused(format!("unsupported schema identifier {name:?}")))
    }
}

fn class_of(table: &str, column: &str) -> Option<FenceClass> {
    CLASSES
        .iter()
        .find(|(t, c, _)| *t == table && *c == column)
        .map(|(_, _, class)| *class)
        .or_else(|| {
            DERIVED_PREFIXES
                .iter()
                .any(|(prefix, c)| table.starts_with(prefix) && *c == column)
                .then_some(FenceClass::Derived)
        })
}

/// The trigger operations a class requires.
fn operations(class: FenceClass) -> &'static [&'static str] {
    match class {
        FenceClass::All => &["insert", "update", "delete"],
        FenceClass::WritesOnly => &["insert", "update"],
        FenceClass::Reference | FenceClass::Derived => &[],
    }
}

fn trigger_name(table: &str, column: &str, op: &str) -> String {
    format!("{TRIGGER_PREFIX}{table}_{column}_{op}")
}

/// SQL predicate: does `value` name a fenced graph? `g` columns hold term
/// ids, resolved at write time so a graph interned later is still covered.
fn names_fenced(column: &str, value: &str) -> String {
    if column == "g" {
        format!("{value} IN (SELECT t.id FROM terms t JOIN {FENCE_TABLE} f ON t.iri = f.iri)")
    } else {
        format!("{value} IN (SELECT iri FROM {FENCE_TABLE})")
    }
}

fn trigger_sql(table: &str, column: &str, op: &str) -> String {
    let when = match op {
        "insert" => names_fenced(column, &format!("NEW.\"{column}\"")),
        "update" => format!(
            "{} OR {}",
            names_fenced(column, &format!("NEW.\"{column}\"")),
            names_fenced(column, &format!("OLD.\"{column}\""))
        ),
        _ => names_fenced(column, &format!("OLD.\"{column}\"")),
    };
    format!(
        "CREATE TRIGGER IF NOT EXISTS \"{name}\" BEFORE {verb} ON \"{table}\" \
         FOR EACH ROW WHEN {when} BEGIN SELECT RAISE(ABORT, '{MESSAGE}'); END;",
        name = trigger_name(table, column, op),
        verb = op.to_ascii_uppercase(),
    )
}

impl Store {
    /// Every graph-bearing column in the live legacy schema, classified.
    /// Refuses an unclassified column or a virtual table that holds one.
    pub fn routed_fence_inventory(&self) -> Result<Vec<FencedColumn>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.name, coalesce(m.sql, ''), p.name \
             FROM main.sqlite_master m JOIN pragma_table_info(m.name) p \
             WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%' AND m.name <> ?1 \
               AND (p.name = 'g' OR lower(p.name) LIKE '%graph%') \
             ORDER BY m.name, p.name",
        )?;
        let rows = stmt
            .query_map([FENCE_TABLE], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(table, sql, column)| {
                let class = class_of(&table, &column).ok_or_else(|| {
                    refused(format!(
                        "unclassified graph-bearing column {table}.{column}; \
                         review its fence class before routing"
                    ))
                })?;
                if sql
                    .trim_start()
                    .to_ascii_uppercase()
                    .starts_with("CREATE VIRTUAL")
                    && !operations(class).is_empty()
                {
                    return Err(refused(format!(
                        "virtual table {table} cannot carry a write fence"
                    )));
                }
                Ok(FencedColumn {
                    table,
                    column,
                    class,
                })
            })
            .collect()
    }

    /// Fence `graph` in this legacy file at the published catalog revision and
    /// owner generation. Installs any missing trigger, then records the graph.
    /// Re-fencing the same graph needs the identical revision and generation.
    pub fn fence_routed_graph(
        &self,
        graph: &str,
        catalog_revision: u64,
        generation: u64,
    ) -> Result<Vec<FencedColumn>> {
        if graph.trim().is_empty()
            || graph == crate::schema::ROOT_GRAPH_IRI
            || catalog_revision == 0
            || generation == 0
        {
            return Err(refused(
                "a fence needs a named graph, catalog revision and owner generation",
            ));
        }
        let revision = i64::try_from(catalog_revision).map_err(refused)?;
        let generation = i64::try_from(generation).map_err(refused)?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {FENCE_TABLE} (
                 iri              TEXT PRIMARY KEY,
                 catalog_revision INTEGER NOT NULL CHECK (catalog_revision > 0),
                 generation       INTEGER NOT NULL CHECK (generation > 0),
                 fenced_at        TEXT NOT NULL
             );"
        ))?;
        let inventory = self.routed_fence_inventory()?;
        for column in &inventory {
            let table = identifier(&column.table)?;
            let name = identifier(&column.column)?;
            for op in operations(column.class) {
                tx.execute_batch(&trigger_sql(table, name, op))?;
            }
        }
        tx.execute(
            &format!(
                "INSERT OR IGNORE INTO {FENCE_TABLE}(iri, catalog_revision, generation, fenced_at) \
                 VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
            ),
            rusqlite::params![graph, revision, generation],
        )?;
        let (have_revision, have_generation): (i64, i64) = tx.query_row(
            &format!("SELECT catalog_revision, generation FROM {FENCE_TABLE} WHERE iri = ?1"),
            [graph],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if (have_revision, have_generation) != (revision, generation) {
            return Err(refused(
                "graph is already fenced at another revision or generation",
            ));
        }
        tx.commit()?;
        Ok(inventory)
    }

    /// Graph IRIs fenced in this file. An absent fence table is none.
    pub fn routed_graphs_fenced(&self) -> Result<Vec<String>> {
        if !self.fence_table_exists()? {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT iri FROM {FENCE_TABLE} ORDER BY iri"))?;
        let graphs = stmt
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(graphs)
    }

    /// Re-derive the inventory and prove every fenced column still has its
    /// triggers. A table created after installation (a new migration, an
    /// on-demand table) refuses until the fence is re-installed over it.
    /// Run before relying on the fence, e.g. at each catalog pin.
    pub fn verify_routed_fence(&self) -> Result<Vec<FencedColumn>> {
        let inventory = self.routed_fence_inventory()?;
        if self.routed_graphs_fenced()?.is_empty() {
            return Ok(inventory);
        }
        for column in &inventory {
            for op in operations(column.class) {
                let name = trigger_name(&column.table, &column.column, op);
                let present: bool = self.conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM main.sqlite_master \
                     WHERE type = 'trigger' AND name = ?1 AND tbl_name = ?2)",
                    rusqlite::params![name, column.table],
                    |r| r.get(0),
                )?;
                if !present {
                    return Err(refused(format!(
                        "fence trigger missing on {}.{} ({op}); re-install before routing",
                        column.table, column.column
                    )));
                }
            }
        }
        Ok(inventory)
    }

    fn fence_table_exists(&self) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type = 'table' AND name = ?1)",
            [FENCE_TABLE],
            |r| r.get(0),
        )?)
    }
}

#[cfg(test)]
#[path = "routed_fence_tests.rs"]
mod tests;
