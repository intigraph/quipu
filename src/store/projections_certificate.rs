//! A compatibility certificate beside a projection aggregate (aegis-eif2sj).
//!
//! A client that normalizes projected rows (Seeds treats a work item as one
//! typed subject with exactly one plain status and identifier) must know the
//! raw table matches that model before it trusts a count. Without this it had
//! to re-read every subject through SPARQL on each call, which cannot finish
//! on a large board. The certificate answers the same questions from the
//! projection table and the current-fact indexes, inside the aggregate's own
//! read snapshot, so its counts and the aggregate share one `tx`.
//!
//! It reports facts, never a verdict: each anomaly is a count, and the caller
//! decides which counts must be zero for its model. The scope is the whole
//! `graph`, independent of the aggregate's filters.

use serde_json::{Value as JsonValue, json};

use super::*;

/// Columns whose non-null values must be unique cannot exceed this many.
const MAX_UNIQUE: usize = 4;

/// SQL: the stored value in `column` is a plain string (tag byte 1).
fn is_plain_sql(column: &str) -> String {
    format!("substr({column}, 1, 1) = X'01'")
}

/// What to certify beside an aggregate.
#[derive(Debug, Clone, Default)]
pub struct ProjectionCertificate<'a> {
    /// Class IRI. Every current member in the graph should hold a key row,
    /// and every key row should belong to a current member.
    pub class: &'a str,
    /// Declared columns whose non-null values must be unique in the graph.
    pub unique: &'a [&'a str],
}

impl Store {
    /// An aggregate and its certificate, answered from one read snapshot.
    /// The aggregate is exactly [`Store::aggregate_projection`]'s answer, with
    /// a `certificate` object added. A certificate needs one `graph`.
    pub fn aggregate_projection_certified(
        &self,
        def: &ProjectionConfig,
        q: &ProjectionAggregate<'_>,
        cert: &ProjectionCertificate<'_>,
    ) -> Result<JsonValue> {
        if !q.graphs.is_empty() {
            return Err(Error::InvalidValue(
                "a certificate covers one graph; use graph, not graphs".into(),
            ));
        }
        if cert.class.trim().is_empty() {
            return Err(Error::InvalidValue("certificate class is required".into()));
        }
        if cert.unique.len() > MAX_UNIQUE {
            return Err(Error::InvalidValue(format!(
                "certificate unique takes at most {MAX_UNIQUE} columns"
            )));
        }
        let mut unique = Vec::with_capacity(cert.unique.len());
        for name in cert.unique {
            let column = def
                .columns
                .iter()
                .find(|c| c.name == *name)
                .ok_or_else(|| {
                    Error::InvalidValue(format!(
                        "certificate unique column {name:?} is not declared on projection {:?}",
                        def.name
                    ))
                })?;
            if column.equals.is_some() {
                return Err(Error::InvalidValue(format!(
                    "certificate unique column {name:?} is a presence column"
                )));
            }
            unique.push(column.name.clone());
        }
        // The same snapshot discipline as the aggregate: one transaction for
        // every statement, so no write lands between the two answers.
        let read = if self.conn.is_autocommit() {
            Some(self.conn.unchecked_transaction()?)
        } else {
            None
        };
        let mut answer = self.aggregate_projection(def, q)?;
        answer["certificate"] = self.certificate_in_snapshot(def, q.graph, cert.class, &unique)?;
        if let Some(read) = read {
            read.commit()?;
        }
        Ok(answer)
    }

    fn certificate_in_snapshot(
        &self,
        def: &ProjectionConfig,
        graph: &str,
        class: &str,
        unique: &[String],
    ) -> Result<JsonValue> {
        // The aggregate answered first in this snapshot has already refused a
        // projection that is not installed at its declared definition.
        let t = table(&def.name);
        let g = if graph.trim().is_empty() || graph == crate::schema::ROOT_GRAPH_IRI {
            Some(crate::schema::ROOT_GRAPH)
        } else {
            self.lookup(graph)?
        };
        // A term that was never interned has no facts: every count is zero,
        // which is a true answer, not an absence of one.
        let type_attr = self.lookup(crate::namespace::RDF_TYPE)?;
        let class_id = self.lookup(class)?;
        let member = match (g, type_attr, class_id) {
            (Some(g), Some(a), Some(c)) => Some((g, a, Value::Ref(c).to_bytes())),
            _ => None,
        };
        let count = |sql: &str, binds: &[rusqlite::types::Value]| -> Result<i64> {
            Ok(self
                .conn
                .query_row(sql, rusqlite::params_from_iter(binds.iter()), |r| r.get(0))?)
        };
        let g_bind = rusqlite::types::Value::from(g.unwrap_or(-1));
        let rows = count(
            &format!("SELECT count(*) FROM {t} WHERE g = ?"),
            std::slice::from_ref(&g_bind),
        )?;
        let key_not_plain = count(
            &format!(
                "SELECT count(*) FROM {t} WHERE g = ? AND NOT ({})",
                is_plain_sql("key")
            ),
            std::slice::from_ref(&g_bind),
        )?;
        let (members, members_without_key, rows_without_class) = match &member {
            Some((g, a, v)) => {
                let binds = [
                    rusqlite::types::Value::from(*g),
                    rusqlite::types::Value::from(*a),
                    rusqlite::types::Value::from(v.clone()),
                ];
                // DISTINCT: one subject may carry the same type fact twice.
                let members = count(
                    "SELECT count(DISTINCT e) FROM facts \
                     WHERE v = ?3 AND g = ?1 AND a = ?2 AND op = 1 AND valid_to IS NULL",
                    &binds,
                )?;
                let without_key = count(
                    &format!(
                        "SELECT count(DISTINCT f.e) FROM facts f \
                         WHERE f.v = ?3 AND f.g = ?1 AND f.a = ?2 AND f.op = 1 AND f.valid_to IS NULL \
                         AND NOT EXISTS (SELECT 1 FROM {t} p WHERE p.g = f.g AND p.e = f.e)"
                    ),
                    &binds,
                )?;
                let without_class = count(
                    &format!(
                        "SELECT count(*) FROM {t} p WHERE p.g = ?1 AND NOT EXISTS (\
                         SELECT 1 FROM facts f WHERE f.v = ?3 AND f.g = p.g AND f.e = p.e \
                         AND f.a = ?2 AND f.op = 1 AND f.valid_to IS NULL)"
                    ),
                    &binds,
                )?;
                (members, without_key, without_class)
            }
            // No such class, type predicate or graph: no members, and every
            // projected row lacks the class.
            None => (0, 0, rows),
        };
        let mut columns = serde_json::Map::new();
        for name in unique {
            let missing = count(
                &format!("SELECT count(*) FROM {t} WHERE g = ? AND {name} IS NULL"),
                std::slice::from_ref(&g_bind),
            )?;
            let not_plain = count(
                &format!(
                    "SELECT count(*) FROM {t} WHERE g = ? AND {name} IS NOT NULL AND NOT ({})",
                    is_plain_sql(name)
                ),
                std::slice::from_ref(&g_bind),
            )?;
            // Rows whose value another row in the graph also holds.
            let duplicated = count(
                &format!(
                    "SELECT coalesce(sum(n), 0) FROM (SELECT count(*) AS n FROM {t} \
                     WHERE g = ? AND {name} IS NOT NULL GROUP BY {name} HAVING count(*) > 1)"
                ),
                std::slice::from_ref(&g_bind),
            )?;
            columns.insert(
                name.clone(),
                json!({"missing": missing, "not_plain": not_plain, "duplicated": duplicated}),
            );
        }
        Ok(json!({
            "scope": "graph",
            "class": class,
            "rows": rows,
            "members": members,
            "members_without_key": members_without_key,
            "rows_without_class": rows_without_class,
            "key_not_plain": key_not_plain,
            "unique": columns,
            "tx": self.latest_tx_id()?,
        }))
    }
}
