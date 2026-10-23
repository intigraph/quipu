//! Answering aggregates over a projection (aegis-tg5 F2): one read snapshot
//! per answer, names checked against the declaration, values bound.

use std::collections::BTreeMap;

use rusqlite::OptionalExtension;
use serde_json::{Value as JsonValue, json};

use super::*;

// Test seam: runs between an aggregate's total and its groups, so a test can
// land a write exactly there (the snapshot test needs a deterministic race).
#[cfg(test)]
thread_local! {
    static AFTER_TOTAL: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn set_after_total(hook: Option<Box<dyn FnMut()>>) {
    AFTER_TOTAL.with(|h| *h.borrow_mut() = hook);
}

/// An aggregate over one projection, in one graph. Column names are checked
/// against the declaration before they reach SQL; values are bound.
#[derive(Debug, Clone, Default)]
pub struct ProjectionAggregate<'a> {
    pub graph: &'a str,
    /// Several graphs answered from one snapshot. When non-empty it replaces
    /// `graph`, and every group carries the `graph` it was counted in, so a
    /// caller can see an entity present in more than one of them.
    pub graphs: &'a [&'a str],
    /// Key values to keep (empty = all). Strings, integers, or `{"iri": ...}`.
    pub key_in: &'a [JsonValue],
    /// Column filters: every column must match, and a column matches any of
    /// its listed values (for example assignee and priority, grouped by
    /// status; aegis-tg5). A `null`
    /// value matches a row with no value for that column; an empty list
    /// matches nothing. Columns are checked against the declaration.
    pub filters: &'a [(&'a str, &'a [JsonValue])],
    /// A declared column (or `"key"`) to group by.
    pub group_by: Option<&'a str>,
    /// `(time column, RFC 3339 instant)`: keep rows whose column is at or after it.
    pub since: Option<(&'a str, &'a str)>,
    /// Cap on returned groups; the answer says `truncated` rather than going partial silently.
    pub max_groups: usize,
}

/// The validated, bound WHERE clause shared by aggregates and row listings.
pub(super) struct Scope {
    pub(super) table: String,
    pub(super) clause: String,
    pub(super) binds: Vec<rusqlite::types::Value>,
    /// Some input can only match nothing. Every input was still validated.
    pub(super) matches_nothing: bool,
    /// Internal graph id -> the IRI the caller named it by.
    pub(super) graph_iris: BTreeMap<i64, String>,
    /// The caller passed `graphs`, so answers say which graph each row is in.
    pub(super) per_graph: bool,
}

pub(super) fn value_json(store: &Store, blob: Option<Vec<u8>>) -> Result<JsonValue> {
    let Some(b) = blob else {
        return Ok(JsonValue::Null);
    };
    Ok(match Value::from_bytes(&b)? {
        Value::Str(s) => json!(s),
        Value::Int(n) => json!(n),
        Value::Ref(id) => json!({ "iri": store.resolve(id)? }),
        Value::Typed { lexical, .. } | Value::Lang { lexical, .. } => json!(lexical),
        Value::Bool(b) => json!(b),
        Value::Float(f) => json!(f),
        Value::Bytes(_) => json!("<bytes>"),
    })
}

/// Whether a stored value is a plain string literal. `value_json` renders a
/// typed or language-tagged literal by its lexical form, so without this a
/// caller could not tell `"open"` from `"open"^^xsd:token`.
pub(super) fn is_plain(blob: Option<&[u8]>) -> Result<bool> {
    Ok(match blob {
        Some(b) => matches!(Value::from_bytes(b)?, Value::Str(_)),
        None => false,
    })
}

fn bind_value(store: &Store, v: &JsonValue) -> Result<Option<Vec<u8>>> {
    Ok(match v {
        JsonValue::String(s) => Some(Value::Str(s.clone()).to_bytes()),
        JsonValue::Number(n) => Some(
            Value::Int(n.as_i64().ok_or_else(|| {
                Error::InvalidValue(format!("filter number {n} is not a 64-bit integer"))
            })?)
            .to_bytes(),
        ),
        JsonValue::Object(o) => match o.get("iri").and_then(JsonValue::as_str) {
            // An unknown IRI cannot match anything; it is not an error.
            Some(iri) => store.lookup(iri)?.map(|id| Value::Ref(id).to_bytes()),
            None => return Err(Error::InvalidValue("filter object needs \"iri\"".into())),
        },
        _ => {
            return Err(Error::InvalidValue(
                "filter values are strings, integers or {\"iri\"}".into(),
            ));
        }
    })
}

/// A declared column name (or `"key"`), never caller text, for use in SQL.
pub(super) fn column(def: &ProjectionConfig, c: &str) -> Result<String> {
    if c == "key" || def.columns.iter().any(|d| d.name == c) {
        Ok(c.to_string())
    } else {
        Err(Error::InvalidValue(format!(
            "projection {:?} has no column {c:?}",
            def.name
        )))
    }
}

/// The inputs both aggregates and row listings restrict by.
pub(super) struct ScopeInput<'a> {
    pub(super) graph: &'a str,
    pub(super) graphs: &'a [&'a str],
    pub(super) key_in: &'a [JsonValue],
    pub(super) filters: &'a [(&'a str, &'a [JsonValue])],
    pub(super) since: Option<(&'a str, &'a str)>,
}

impl Store {
    /// Validate the declaration is installed and build the bound WHERE clause.
    ///
    /// Some inputs can only match nothing (an unknown graph, an empty
    /// alternative list, only unknown IRIs). That is recorded rather than
    /// returned early, so EVERY input is still validated: an empty filter must
    /// not turn a later undeclared column or bad value into a successful zero.
    pub(super) fn projection_scope(
        &self,
        def: &ProjectionConfig,
        s: &ScopeInput<'_>,
    ) -> Result<Scope> {
        validate(def)?;
        let installed: Option<String> = self
            .conn
            .query_row(
                "SELECT def_hash FROM projection_defs WHERE name = ?1",
                [&def.name],
                |r| r.get(0),
            )
            .optional()
            .or_else(|e| match e {
                // No projection has ever been installed in this store.
                rusqlite::Error::SqliteFailure(..) => Ok(None),
                other => Err(other),
            })?;
        if installed.as_deref() != Some(def_hash(def).as_str()) {
            return Err(Error::CannotVerify(format!(
                "projection {:?} is not installed at its declared definition; answers would be stale",
                def.name
            )));
        }
        let per_graph = !s.graphs.is_empty();
        if s.graphs.len() > 4 {
            return Err(Error::InvalidValue("graphs takes at most 4 graphs".into()));
        }
        let named: Vec<&str> = if per_graph {
            s.graphs.to_vec()
        } else {
            vec![s.graph]
        };
        let mut matches_nothing = false;
        let mut graph_iris = BTreeMap::new();
        for iri in named {
            let g = if iri.trim().is_empty() || iri == crate::schema::ROOT_GRAPH_IRI {
                Some(crate::schema::ROOT_GRAPH)
            } else {
                self.lookup(iri)?
            };
            if let Some(g) = g {
                graph_iris.insert(g, iri.to_string());
            }
        }
        let mut wh = Vec::new();
        let mut binds: Vec<rusqlite::types::Value> = Vec::new();
        if graph_iris.is_empty() {
            matches_nothing = true;
        } else {
            wh.push(format!("g IN ({})", vec!["?"; graph_iris.len()].join(",")));
            binds.extend(graph_iris.keys().map(|g| rusqlite::types::Value::from(*g)));
        }
        if !s.key_in.is_empty() {
            let vals: Vec<Vec<u8>> = s
                .key_in
                .iter()
                .map(|v| bind_value(self, v))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect();
            if vals.is_empty() {
                matches_nothing = true;
            } else {
                wh.push(format!("key IN ({})", vec!["?"; vals.len()].join(",")));
                binds.extend(vals.into_iter().map(rusqlite::types::Value::Blob));
            }
        }
        for (c, values) in s.filters {
            let c = column(def, c)?;
            let mut alternatives = Vec::new();
            for v in *values {
                if v.is_null() {
                    alternatives.push(format!("{c} IS NULL"));
                } else if let Some(b) = bind_value(self, v)? {
                    alternatives.push(format!("{c} = ?"));
                    binds.push(rusqlite::types::Value::Blob(b));
                }
                // An unknown IRI binds nothing and so cannot match.
            }
            if alternatives.is_empty() {
                matches_nothing = true;
            } else {
                wh.push(format!("({})", alternatives.join(" OR ")));
            }
        }
        if let Some((c, at)) = s.since {
            let c = column(def, c)?;
            if !def.columns.iter().any(|d| d.name == c && d.time) {
                return Err(Error::InvalidValue(format!(
                    "column {c:?} is not a time column"
                )));
            }
            let epoch = crate::time::epoch_of_rfc3339(at).ok_or_else(|| {
                Error::InvalidValue(format!("since: {at:?} is not an RFC 3339 instant"))
            })?;
            wh.push(format!("{c}_t >= ?"));
            binds.push(epoch.into());
        }
        Ok(Scope {
            table: table(&def.name),
            clause: if wh.is_empty() {
                "1".into()
            } else {
                wh.join(" AND ")
            },
            binds,
            matches_nothing,
            graph_iris,
            per_graph,
        })
    }

    /// Count rows of a projection, optionally grouped. Returns
    /// `{total, groups:[{value, n, plain}], truncated, tx}` where `tx` is the
    /// store's latest transaction when the answer was read (the watermark a
    /// cache can invalidate on). With `graphs`, each group also carries its
    /// `graph`.
    pub fn aggregate_projection(
        &self,
        def: &ProjectionConfig,
        q: &ProjectionAggregate<'_>,
    ) -> Result<JsonValue> {
        // One read snapshot for every statement below (installed check, total,
        // groups, multi-valued count, tx), so a write landing between them
        // cannot pair a total from one state with groups or a tx from another.
        // A caller already inside a transaction is already on one snapshot.
        if !self.conn.is_autocommit() {
            return self.aggregate_projection_in_snapshot(def, q);
        }
        let read = self.conn.unchecked_transaction()?;
        let answer = self.aggregate_projection_in_snapshot(def, q)?;
        read.commit()?;
        Ok(answer)
    }

    fn aggregate_projection_in_snapshot(
        &self,
        def: &ProjectionConfig,
        q: &ProjectionAggregate<'_>,
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
        let group_column = q.group_by.map(|c| column(def, c)).transpose()?;
        if scope.matches_nothing {
            return Ok(
                json!({ "total": 0, "groups": [], "truncated": false, "tx": self.latest_tx_id()? }),
            );
        }
        let Scope {
            table: t,
            clause: wh,
            binds,
            graph_iris,
            per_graph,
            ..
        } = scope;
        let total: i64 = self.conn.query_row(
            &format!("SELECT count(*) FROM {t} WHERE {wh}"),
            rusqlite::params_from_iter(binds.iter()),
            |r| r.get(0),
        )?;
        #[cfg(test)]
        AFTER_TOTAL.with(|hook| {
            if let Some(f) = hook.borrow_mut().as_mut() {
                f();
            }
        });
        let mut groups = Vec::new();
        let mut truncated = false;
        if let Some(c) = group_column {
            let cap = if q.max_groups == 0 {
                1000
            } else {
                q.max_groups
            };
            // Per-graph answers group by (graph, value): the same value in two
            // graphs is two groups, which is what lets a caller see a shadow.
            let by = if per_graph {
                format!("g, {c}")
            } else {
                c.clone()
            };
            let mut st = self.conn.prepare(&format!(
                "SELECT g, {c}, count(*) FROM {t} WHERE {wh} GROUP BY {by} ORDER BY count(*) DESC LIMIT {}",
                cap + 1
            ))?;
            let rows: Vec<(i64, Option<Vec<u8>>, i64)> = st
                .query_map(rusqlite::params_from_iter(binds.iter()), |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            truncated = rows.len() > cap;
            for (g, v, n) in rows.into_iter().take(cap) {
                let plain = is_plain(v.as_deref())?;
                let mut group = json!({ "value": value_json(self, v)?, "n": n, "plain": plain });
                if per_graph {
                    group["graph"] = json!(graph_iris.get(&g));
                }
                groups.push(group);
            }
        }
        let multi: i64 = self.conn.query_row(
            &format!("SELECT count(*) FROM {t} WHERE {wh} AND multi"),
            rusqlite::params_from_iter(binds.iter()),
            |r| r.get(0),
        )?;
        Ok(
            json!({ "total": total, "groups": groups, "truncated": truncated, "multi_valued_rows": multi, "tx": self.latest_tx_id()? }),
        )
    }
}
