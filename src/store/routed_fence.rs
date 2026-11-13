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
    ("fluent_config_receipts", "g", FenceClass::All),
    ("fluent_origin_receipts", "g", FenceClass::All),
    // Native append removes these transaction-local permits on completion.
    ("fluent_write_permits", "g", FenceClass::WritesOnly),
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
const SOURCE_FENCE_TABLE: &str = "routed_source_fence";
const SOURCE_TRIGGERS: [&str; 2] = [
    "quipu_routed_fence_transactions_source_insert",
    "quipu_routed_fence_transactions_source_update",
];
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
             WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%' AND m.name NOT IN (?1, ?2) \
               AND (p.name = 'g' OR lower(p.name) LIKE '%graph%') \
             ORDER BY m.name, p.name",
        )?;
        let rows = stmt
            .query_map([FENCE_TABLE, SOURCE_FENCE_TABLE], |r| {
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

    /// Each fenced graph with the catalog revision and owner generation it
    /// was fenced at. An absent fence table is none.
    pub fn routed_graph_fence_rows(&self) -> Result<Vec<(String, u64, u64)>> {
        if !self.fence_table_exists()? {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(&format!(
            "SELECT iri, catalog_revision, generation FROM {FENCE_TABLE} ORDER BY iri"
        ))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(iri, revision, generation)| {
                Ok((
                    iri,
                    u64::try_from(revision).map_err(refused)?,
                    u64::try_from(generation).map_err(refused)?,
                ))
            })
            .collect()
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
        if !self.routed_source_fence_rows()?.is_empty() {
            for name in SOURCE_TRIGGERS {
                let present: bool = self.conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM main.sqlite_master \
                     WHERE type = 'trigger' AND name = ?1 AND tbl_name = 'transactions')",
                    [name],
                    |r| r.get(0),
                )?;
                if !present {
                    return Err(refused(format!(
                        "producer source fence trigger {name} missing; re-install before routing"
                    )));
                }
            }
        }
        Ok(inventory)
    }

    /// Move one producer's ROOT facts behind the fence, atomically (cutover).
    ///
    /// Ingest producers write ROOT today and own their facts by the
    /// `transactions.source` tag `snapshot:<key>`. A graph fence cannot stop
    /// an old or misconfigured producer that keeps omitting `graph`, so the
    /// legacy file also refuses any new transaction under a moved tag. In ONE
    /// savepoint this retracts that tag's current ROOT facts (bitemporal: the
    /// history stays readable) and records the tag. Otherwise ROOT consumers
    /// would keep seeing stale copies the producer can no longer replace, or a
    /// producer write between the two steps could re-create them.
    ///
    /// `graph` must already be fenced at the same revision and generation.
    /// Re-running with identical arguments retracts nothing and succeeds.
    /// Returns the number of retractions written.
    pub fn cut_over_routed_source(
        &mut self,
        source_tag: &str,
        graph: &str,
        catalog_revision: u64,
        generation: u64,
        timestamp: &str,
    ) -> Result<usize> {
        if source_tag
            .strip_prefix("snapshot:")
            .is_none_or(|key| key.trim().is_empty())
        {
            return Err(refused("a producer source tag is `snapshot:<key>`"));
        }
        if !self.routed_graph_fence_rows()?.contains(&(
            graph.to_string(),
            catalog_revision,
            generation,
        )) {
            return Err(refused(
                "fence the routed graph at this revision and generation first",
            ));
        }
        if let Some(row) = self
            .routed_source_fence_rows()?
            .into_iter()
            .find(|row| row.0 == source_tag)
        {
            return if row
                == (
                    source_tag.to_string(),
                    graph.to_string(),
                    catalog_revision,
                    generation,
                ) {
                Ok(0)
            } else {
                Err(refused(
                    "producer source is already fenced for another route",
                ))
            };
        }
        let revision = i64::try_from(catalog_revision).map_err(refused)?;
        let generation = i64::try_from(generation).map_err(refused)?;
        self.ensure_ownership_writable()?;
        self.conn.execute_batch("SAVEPOINT quipu_source_cutover")?;
        let result =
            self.cut_over_routed_source_inner(source_tag, graph, revision, generation, timestamp);
        match result {
            Ok(count) => {
                self.conn.execute_batch("RELEASE quipu_source_cutover")?;
                Ok(count)
            }
            Err(error) => {
                crate::store::release::rollback_release(&self.conn, "quipu_source_cutover")?;
                self.read_model.borrow_mut().clear();
                Err(error)
            }
        }
    }

    fn cut_over_routed_source_inner(
        &mut self,
        source_tag: &str,
        graph: &str,
        revision: i64,
        generation: i64,
        timestamp: &str,
    ) -> Result<usize> {
        // The triggers are inert until the tag row exists, so the retraction
        // below (written under the same tag) is still admitted.
        self.conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {SOURCE_FENCE_TABLE} (
                 source           TEXT PRIMARY KEY,
                 graph            TEXT NOT NULL,
                 catalog_revision INTEGER NOT NULL CHECK (catalog_revision > 0),
                 generation       INTEGER NOT NULL CHECK (generation > 0),
                 fenced_at        TEXT NOT NULL
             );
             CREATE TRIGGER IF NOT EXISTS \"{insert}\" BEFORE INSERT ON transactions
             FOR EACH ROW WHEN NEW.source IN (SELECT source FROM {SOURCE_FENCE_TABLE})
             BEGIN SELECT RAISE(ABORT, '{MESSAGE}'); END;
             CREATE TRIGGER IF NOT EXISTS \"{update}\" BEFORE UPDATE OF source ON transactions
             FOR EACH ROW WHEN NEW.source IN (SELECT source FROM {SOURCE_FENCE_TABLE})
             BEGIN SELECT RAISE(ABORT, '{MESSAGE}'); END;",
            insert = SOURCE_TRIGGERS[0],
            update = SOURCE_TRIGGERS[1],
        ))?;
        let datums = self.plan_source_retraction(source_tag, 0)?;
        let count = datums.len();
        if count > 0 {
            self.transact_to_graph_scoped(
                &datums,
                timestamp,
                None,
                Some(source_tag),
                0,
                Some(source_tag),
            )?;
        }
        self.conn.execute(
            &format!(
                "INSERT INTO {SOURCE_FENCE_TABLE}(source, graph, catalog_revision, generation, fenced_at) \
                 VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
            ),
            rusqlite::params![source_tag, graph, revision, generation],
        )?;
        Ok(count)
    }

    /// Fenced producer source tags with their routed graph, catalog revision
    /// and owner generation. An absent table is none.
    pub fn routed_source_fence_rows(&self) -> Result<Vec<(String, String, u64, u64)>> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type = 'table' AND name = ?1)",
            [SOURCE_FENCE_TABLE],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(&format!(
            "SELECT source, graph, catalog_revision, generation FROM {SOURCE_FENCE_TABLE} ORDER BY source"
        ))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(source, graph, revision, generation)| {
                Ok((
                    source,
                    graph,
                    u64::try_from(revision).map_err(refused)?,
                    u64::try_from(generation).map_err(refused)?,
                ))
            })
            .collect()
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
