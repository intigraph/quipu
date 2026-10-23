//! Write-maintained current-state projections (aegis-tg5 F2).
//!
//! A projection is a narrow table with one row per (graph, entity) that
//! currently has a value for its `key` predicate, holding the current value of
//! each declared column. Board counts and "closed since T" become index reads
//! over tens of thousands of narrow rows instead of SPARQL joins over the facts
//! of every item.
//!
//! MAINTAINED BY TRIGGERS ON `facts`, deliberately. Facts are written from many
//! places (the ops path, supersede, retraction, import, forks, overlays,
//! freeze I/O, source claims, denial replay); a projection maintained in Rust
//! at each of them would drift the first time one was missed. A trigger fires
//! for every one of them, inside the writing transaction, so a committed write
//! and its projection row commit or roll back together.
//!
//! NEVER A SECOND SOURCE OF TRUTH. The table is a deterministic function of the
//! current facts: [`Store::rebuild_projection`] recomputes it, and
//! [`Store::check_projection`] diffs it against a fresh recompute. The
//! definition hash is stored, so a changed declaration rebuilds on install.
//!
//! SCOPE: the main store only. Facts in attached packs are not projected; a
//! caller that needs them reads through the ordinary query path.

use std::collections::BTreeMap;

use rusqlite::params;
use serde_json::{Value as JsonValue, json};

use super::Store;
use crate::config::ProjectionConfig;
use crate::types::Value;
use crate::{Error, Result};

const RESERVED: &[&str] = &["g", "e", "key"];
/// The `xsd:dateTime` typed-literal prefix: tag, little-endian length, IRI.
const XSD_DATETIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";

fn valid_ident(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 48
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !s.as_bytes()[0].is_ascii_digit()
}

fn table(name: &str) -> String {
    format!("qp_proj_{name}")
}

/// Reject a declaration before any DDL is generated from it: every name that
/// reaches SQL text is checked against a closed character set here.
pub(crate) fn validate(def: &ProjectionConfig) -> Result<()> {
    let bad = |m: String| {
        Err(Error::InvalidValue(format!(
            "projection {:?}: {m}",
            def.name
        )))
    };
    if !valid_ident(&def.name) {
        return bad("name must be lowercase letters, digits and _".into());
    }
    if def.key.is_empty() {
        return bad("key predicate is required".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for c in &def.columns {
        if !valid_ident(&c.name) || RESERVED.contains(&c.name.as_str()) {
            return bad(format!(
                "column {:?} must be lowercase letters, digits and _, not g/e/key",
                c.name
            ));
        }
        if !seen.insert(c.name.as_str()) {
            return bad(format!("column {:?} declared twice", c.name));
        }
        if c.predicate.is_empty() {
            return bad(format!("column {:?} needs a predicate", c.name));
        }
    }
    Ok(())
}

fn def_hash(def: &ProjectionConfig) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    def.name.hash(&mut h);
    def.key.hash(&mut h);
    for c in &def.columns {
        (&c.name, &c.predicate, c.time).hash(&mut h);
    }
    // The trigger body is part of the definition: bump when it changes.
    "v1".hash(&mut h);
    format!("{:016x}", h.finish())
}

/// SQL expression: UTC epoch seconds of a stored value, or NULL. Exact for a
/// typed `xsd:dateTime` literal and a plain string; SQLite's date parser
/// accepts `Z` and `±HH:MM` offsets, so this is offset-safe, and an unparsable
/// value is NULL rather than a wrong time.
fn epoch_sql(v: &str) -> String {
    let prefix = Value::Typed {
        lexical: String::new(),
        datatype: XSD_DATETIME.into(),
    }
    .to_bytes();
    let hex: String = prefix.iter().map(|b| format!("{b:02X}")).collect();
    let n = prefix.len();
    format!(
        "CASE WHEN substr({v},1,{n}) = X'{hex}' THEN CAST(strftime('%s', CAST(substr({v},{s}) AS TEXT)) AS INTEGER) \
              WHEN substr({v},1,1) = X'01' THEN CAST(strftime('%s', CAST(substr({v},2) AS TEXT)) AS INTEGER) \
              ELSE NULL END",
        s = n + 1
    )
}

/// The recompute of one (g, e) from current facts, as a SELECT yielding zero
/// rows (no current key) or one. `ge` names the expressions for g and e.
fn recompute_select(
    attrs: &BTreeMap<String, i64>,
    def: &ProjectionConfig,
    g: &str,
    e: &str,
) -> String {
    let cur = |a: i64| {
        format!(
            "(SELECT v FROM facts WHERE g = {g} AND e = {e} AND a = {a} AND op = 1 AND valid_to IS NULL ORDER BY tx DESC LIMIT 1)"
        )
    };
    let multi = |a: i64| {
        format!(
            "(SELECT count(*) > 1 FROM facts WHERE g = {g} AND e = {e} AND a = {a} AND op = 1 AND valid_to IS NULL)"
        )
    };
    let mut cols = vec![
        format!("{g} AS g"),
        format!("{e} AS e"),
        format!("{} AS key", cur(attrs["key"])),
    ];
    let mut flags = vec![multi(attrs["key"])];
    for c in &def.columns {
        let a = attrs[&c.name];
        cols.push(format!("{} AS {}", cur(a), c.name));
        if c.time {
            cols.push(format!("{} AS {}_t", epoch_sql(&cur(a)), c.name));
        }
        flags.push(multi(a));
    }
    cols.push(format!("({}) AS multi", flags.join(" OR ")));
    format!(
        "SELECT * FROM (SELECT {}) WHERE key IS NOT NULL",
        cols.join(", ")
    )
}

/// What [`Store::install_projections`] did for each declaration.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProjectionStatus {
    pub name: String,
    /// `"unchanged"`, `"built"` (new or changed declaration) or `"dropped"`.
    pub action: String,
    pub rows: i64,
}

impl Store {
    /// Record the declared projections (the server calls this from config).
    pub fn set_projection_defs(&mut self, defs: Vec<ProjectionConfig>) {
        self.projection_defs = defs;
    }

    /// Every declared projection.
    #[must_use]
    pub fn projection_defs_declared(&self) -> &[ProjectionConfig] {
        &self.projection_defs
    }

    /// Installed projections as `[{name, def_hash, built_at, rows}]`; empty when
    /// none has ever been installed.
    pub fn installed_projections(&self) -> Result<Vec<JsonValue>> {
        let exists: bool = self.conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'projection_defs'",
            [],
            |r| r.get::<_, i64>(0),
        )? > 0;
        if !exists {
            return Ok(Vec::new());
        }
        let mut st = self
            .conn
            .prepare("SELECT name, def_hash, built_at, rows FROM projection_defs ORDER BY name")?;
        let rows = st
            .query_map([], |r| {
                Ok(json!({"name": r.get::<_, String>(0)?, "def_hash": r.get::<_, String>(1)?, "built_at": r.get::<_, String>(2)?, "rows": r.get::<_, i64>(3)?}))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// A declared projection by name.
    #[must_use]
    pub fn projection_def(&self, name: &str) -> Option<&ProjectionConfig> {
        self.projection_defs.iter().find(|d| d.name == name)
    }

    /// Make the declared projections exist and be current. Idempotent: an
    /// unchanged declaration is left alone; a new or changed one is (re)built
    /// from facts; a previously installed projection that is no longer declared
    /// is DROPPED, so an undeclared table never keeps taxing writes. Runs in one
    /// transaction: a failed install leaves the previous state.
    pub fn install_projections(&self, defs: &[ProjectionConfig]) -> Result<Vec<ProjectionStatus>> {
        for d in defs {
            validate(d)?;
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS projection_defs (
                 name TEXT PRIMARY KEY, def_hash TEXT NOT NULL, built_at TEXT NOT NULL, rows INTEGER NOT NULL)",
        )?;
        let mut out = Vec::new();
        let installed: Vec<(String, String)> = {
            let mut st = tx.prepare("SELECT name, def_hash FROM projection_defs")?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?
        };
        for (name, _) in &installed {
            if !defs.iter().any(|d| &d.name == name) {
                Self::drop_projection_objects(&tx, name)?;
                tx.execute("DELETE FROM projection_defs WHERE name = ?1", [name])?;
                out.push(ProjectionStatus {
                    name: name.clone(),
                    action: "dropped".into(),
                    rows: 0,
                });
            }
        }
        for d in defs {
            let hash = def_hash(d);
            let current = installed
                .iter()
                .find(|(n, _)| n == &d.name)
                .map(|(_, h)| h.as_str());
            if current == Some(hash.as_str()) {
                let rows: i64 = tx.query_row(
                    &format!("SELECT count(*) FROM {}", table(&d.name)),
                    [],
                    |r| r.get(0),
                )?;
                out.push(ProjectionStatus {
                    name: d.name.clone(),
                    action: "unchanged".into(),
                    rows,
                });
                continue;
            }
            Self::drop_projection_objects(&tx, &d.name)?;
            let rows = self.create_projection(&tx, d)?;
            tx.execute(
                "INSERT OR REPLACE INTO projection_defs (name, def_hash, built_at, rows) VALUES (?1, ?2, ?3, ?4)",
                params![d.name, hash, crate::time::now_iso(), rows],
            )?;
            out.push(ProjectionStatus {
                name: d.name.clone(),
                action: "built".into(),
                rows,
            });
        }
        tx.commit()?;
        Ok(out)
    }

    fn drop_projection_objects(tx: &rusqlite::Transaction<'_>, name: &str) -> Result<()> {
        let t = table(name);
        tx.execute_batch(&format!(
            "DROP TRIGGER IF EXISTS {t}_ins; DROP TRIGGER IF EXISTS {t}_upd; DROP TRIGGER IF EXISTS {t}_del; DROP TABLE IF EXISTS {t};"
        ))?;
        Ok(())
    }

    fn projection_attrs(&self, def: &ProjectionConfig) -> Result<BTreeMap<String, i64>> {
        let mut attrs = BTreeMap::new();
        attrs.insert("key".to_string(), self.intern(&def.key)?);
        for c in &def.columns {
            attrs.insert(c.name.clone(), self.intern(&c.predicate)?);
        }
        Ok(attrs)
    }

    fn create_projection(
        &self,
        tx: &rusqlite::Transaction<'_>,
        def: &ProjectionConfig,
    ) -> Result<i64> {
        let t = table(&def.name);
        let attrs = self.projection_attrs(def)?;
        let mut cols = vec![
            "g INTEGER NOT NULL".to_string(),
            "e INTEGER NOT NULL".into(),
            "key BLOB".into(),
        ];
        let mut idx = vec![format!("CREATE INDEX {t}_key ON {t} (g, key)")];
        for c in &def.columns {
            cols.push(format!("{} BLOB", c.name));
            idx.push(format!(
                "CREATE INDEX {t}_{n} ON {t} (g, key, {n})",
                n = c.name
            ));
            if c.time {
                cols.push(format!("{}_t INTEGER", c.name));
                idx.push(format!(
                    "CREATE INDEX {t}_{n}_t ON {t} (g, key, {n}_t)",
                    n = c.name
                ));
            }
        }
        cols.push("multi INTEGER NOT NULL DEFAULT 0".into());
        tx.execute_batch(&format!(
            "CREATE TABLE {t} ({}, PRIMARY KEY (g, e)) WITHOUT ROWID; {};",
            cols.join(", "),
            idx.join("; ")
        ))?;
        // Initial build: every (g, e) with a current key value.
        Self::fill_projection(tx, &t, def, &attrs)?;
        let ids = attrs
            .values()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let body = format!(
            "DELETE FROM {t} WHERE g = NEW.g AND e = NEW.e; INSERT INTO {t} {};",
            recompute_select(&attrs, def, "NEW.g", "NEW.e")
        );
        tx.execute_batch(&format!(
            "CREATE TRIGGER {t}_ins AFTER INSERT ON facts WHEN NEW.a IN ({ids}) BEGIN {body} END;
             CREATE TRIGGER {t}_upd AFTER UPDATE OF valid_to, op, v ON facts WHEN NEW.a IN ({ids}) OR OLD.a IN ({ids}) BEGIN {body} END;
             CREATE TRIGGER {t}_del AFTER DELETE ON facts WHEN OLD.a IN ({ids}) BEGIN
                 DELETE FROM {t} WHERE g = OLD.g AND e = OLD.e;
                 INSERT INTO {t} {old};
             END;",
            old = recompute_select(&attrs, def, "OLD.g", "OLD.e")
        ))?;
        Ok(tx.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))?)
    }

    /// Fill `target` with one recompute per distinct (g, e) carrying a current
    /// key. Row at a time because SQLite has no LATERAL join; each recompute is
    /// a handful of indexed point reads, so this is linear in the key count.
    fn fill_projection(
        tx: &rusqlite::Transaction<'_>,
        target: &str,
        def: &ProjectionConfig,
        attrs: &BTreeMap<String, i64>,
    ) -> Result<()> {
        let insert = format!(
            "INSERT INTO {target} {}",
            recompute_select(attrs, def, "?1", "?2")
        );
        let mut ins = tx.prepare(&insert)?;
        let mut keys = tx.prepare(
            "SELECT DISTINCT g, e FROM facts WHERE a = ?1 AND op = 1 AND valid_to IS NULL",
        )?;
        let pairs: Vec<(i64, i64)> = keys
            .query_map([attrs["key"]], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (g, e) in pairs {
            ins.execute(params![g, e])?;
        }
        Ok(())
    }

    /// Recompute a projection from facts (the disposable-copy guarantee).
    pub fn rebuild_projection(&self, def: &ProjectionConfig) -> Result<i64> {
        validate(def)?;
        let tx = self.conn.unchecked_transaction()?;
        Self::drop_projection_objects(&tx, &def.name)?;
        let rows = self.create_projection(&tx, def)?;
        tx.execute(
            "INSERT OR REPLACE INTO projection_defs (name, def_hash, built_at, rows) VALUES (?1, ?2, ?3, ?4)",
            params![def.name, def_hash(def), crate::time::now_iso(), rows],
        )?;
        tx.commit()?;
        Ok(rows)
    }

    /// Rows that differ between the projection and a fresh recompute, in
    /// either direction. 0 means the projection is exact.
    pub fn check_projection(&self, def: &ProjectionConfig) -> Result<i64> {
        validate(def)?;
        let attrs = self.projection_attrs(def)?;
        let t = table(&def.name);
        // Recompute into a temp table inside a transaction that is rolled
        // back, then a two-way EXCEPT against the maintained table.
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(&format!(
            "DROP TABLE IF EXISTS temp.qp_check; CREATE TEMP TABLE qp_check AS SELECT * FROM {t} WHERE 0;"
        ))?;
        Self::fill_projection(&tx, "temp.qp_check", def, &attrs)?;
        let diff: i64 = tx.query_row(
            &format!(
                "SELECT (SELECT count(*) FROM (SELECT * FROM {t} EXCEPT SELECT * FROM temp.qp_check)) + \
                        (SELECT count(*) FROM (SELECT * FROM temp.qp_check EXCEPT SELECT * FROM {t}))"
            ),
            [],
            |r| r.get(0),
        )?;
        tx.execute_batch("DROP TABLE temp.qp_check;")?;
        tx.rollback()?;
        Ok(diff)
    }
}

#[path = "projections_aggregate.rs"]
mod aggregate;
pub use aggregate::ProjectionAggregate;
#[path = "projections_rows.rs"]
mod rows;
#[cfg(test)]
use aggregate::set_after_total;
pub use rows::{PROJECTION_ROWS_MAX, ProjectionRows};

#[cfg(test)]
#[path = "projections_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "projections_rows_tests.rs"]
mod rows_tests;
