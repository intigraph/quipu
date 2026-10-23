//! Listing the rows of a projection (aegis-tg5, aegis-0ztg): the entities a
//! filtered current-state question selects, from one read snapshot.

use serde_json::{Value as JsonValue, json};

use super::aggregate::{ScopeInput, column, is_plain, value_json};
use super::*;

/// Largest listing one answer returns. A bigger selection answers
/// `truncated` and the caller must narrow it; there is no paging, because a
/// later page could not be read from the same snapshot as the first.
pub const PROJECTION_ROWS_MAX: usize = 1000;

/// A row listing over one projection. Scoping is identical to
/// [`ProjectionAggregate`](super::ProjectionAggregate).
#[derive(Debug, Clone, Default)]
pub struct ProjectionRows<'a> {
    pub graph: &'a str,
    pub graphs: &'a [&'a str],
    pub key_in: &'a [JsonValue],
    pub filters: &'a [(&'a str, &'a [JsonValue])],
    pub since: Option<(&'a str, &'a str)>,
    /// Declared columns to return; `None` returns every declared column.
    pub columns: Option<&'a [&'a str]>,
    /// 1..=[`PROJECTION_ROWS_MAX`]; 0 means the maximum.
    pub limit: usize,
}

impl Store {
    /// List the rows a scope selects. Returns `{rows:[{entity, graph, key,
    /// key_plain, columns, plain, multi}], truncated, multi_valued_rows, tx}`,
    /// rows in a stable entity order, every value encoded as in aggregates.
    /// `plain` maps each returned column to whether its value is a plain
    /// string literal, so a typed or tagged literal is never mistaken for one.
    pub fn projection_rows(
        &self,
        def: &ProjectionConfig,
        q: &ProjectionRows<'_>,
    ) -> Result<JsonValue> {
        if !self.conn.is_autocommit() {
            return self.projection_rows_in_snapshot(def, q);
        }
        let read = self.conn.unchecked_transaction()?;
        let answer = self.projection_rows_in_snapshot(def, q)?;
        read.commit()?;
        Ok(answer)
    }

    fn projection_rows_in_snapshot(
        &self,
        def: &ProjectionConfig,
        q: &ProjectionRows<'_>,
    ) -> Result<JsonValue> {
        let scope = self.projection_scope(
            def,
            &ScopeInput {
                graph: q.graph,
                graphs: q.graphs,
                key_in: q.key_in,
                filters: q.filters,
                since: q.since,
            },
        )?;
        // Validate the listing's own inputs before any empty answer.
        let selected: Vec<String> = match q.columns {
            Some(cols) => cols
                .iter()
                .map(|c| {
                    let c = column(def, c)?;
                    if c == "key" {
                        return Err(Error::InvalidValue(
                            "the key is always returned; list declared columns only".into(),
                        ));
                    }
                    Ok(c)
                })
                .collect::<Result<_>>()?,
            None => def.columns.iter().map(|c| c.name.clone()).collect(),
        };
        let limit = match q.limit {
            0 => PROJECTION_ROWS_MAX,
            n if n <= PROJECTION_ROWS_MAX => n,
            n => {
                return Err(Error::InvalidValue(format!(
                    "limit {n} exceeds {PROJECTION_ROWS_MAX}"
                )));
            }
        };
        if scope.matches_nothing {
            return Ok(
                json!({ "rows": [], "truncated": false, "multi_valued_rows": 0, "tx": self.latest_tx_id()? }),
            );
        }
        let t = &scope.table;
        let wh = &scope.clause;
        let cols = selected
            .iter()
            .map(|c| format!(", {c}"))
            .collect::<String>();
        let mut st = self.conn.prepare(&format!(
            "SELECT e, g, key, multi{cols} FROM {t} WHERE {wh} ORDER BY e, g LIMIT {}",
            limit + 1
        ))?;
        type Row = (i64, i64, Option<Vec<u8>>, bool, Vec<Option<Vec<u8>>>);
        let raw: Vec<Row> = st
            .query_map(rusqlite::params_from_iter(scope.binds.iter()), |r| {
                let values = (0..selected.len())
                    .map(|i| r.get(4 + i))
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, values))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let truncated = raw.len() > limit;
        let mut rows = Vec::with_capacity(raw.len().min(limit));
        for (e, g, key, multi, values) in raw.into_iter().take(limit) {
            let mut columns = serde_json::Map::new();
            let mut plain = serde_json::Map::new();
            for (name, v) in selected.iter().zip(values) {
                plain.insert(name.clone(), json!(is_plain(v.as_deref())?));
                columns.insert(name.clone(), value_json(self, v)?);
            }
            rows.push(json!({
                "entity": self.resolve(e)?,
                "graph": scope.graph_iris.get(&g),
                "key_plain": is_plain(key.as_deref())?,
                "key": value_json(self, key)?,
                "columns": columns,
                "plain": plain,
                "multi": multi,
            }));
        }
        let multi: i64 = self.conn.query_row(
            &format!("SELECT count(*) FROM {t} WHERE {wh} AND multi"),
            rusqlite::params_from_iter(scope.binds.iter()),
            |r| r.get(0),
        )?;
        Ok(json!({
            "rows": rows,
            "truncated": truncated,
            "multi_valued_rows": multi,
            "tx": self.latest_tx_id()?,
        }))
    }
}
