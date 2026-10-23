//! Answering aggregates over a projection (aegis-tg5 F2): one read snapshot
//! per answer, names checked against the declaration, values bound.

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

fn value_json(store: &Store, blob: Option<Vec<u8>>) -> Result<JsonValue> {
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

fn bind_value(store: &Store, v: &JsonValue) -> Result<Option<Vec<u8>>> {
    Ok(match v {
        JsonValue::String(s) => Some(Value::Str(s.clone()).to_bytes()),
        JsonValue::Number(n) => n.as_i64().map(|n| Value::Int(n).to_bytes()),
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

impl Store {
    /// Count rows of a projection, optionally grouped. Returns
    /// `{total, groups:[{value, n}], truncated, tx}` where `tx` is the store's
    /// latest transaction when the answer was read (the watermark a cache can
    /// invalidate on).
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
        validate(def)?;
        let t = table(&def.name);
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
        // Some inputs can only match nothing (an unknown graph, an empty
        // alternative list, only unknown IRIs). The answer is then zero, but
        // only after EVERY other input has been validated: an empty filter
        // must not turn a later undeclared column or bad value into a
        // successful zero.
        let mut matches_nothing = false;
        let g = if q.graph.trim().is_empty() || q.graph == crate::schema::ROOT_GRAPH_IRI {
            crate::schema::ROOT_GRAPH
        } else if let Some(g) = self.lookup(q.graph)? {
            g
        } else {
            matches_nothing = true;
            crate::schema::ROOT_GRAPH
        };
        let column = |c: &str| -> Result<String> {
            if c == "key" || def.columns.iter().any(|d| d.name == c) {
                Ok(c.to_string())
            } else {
                Err(Error::InvalidValue(format!(
                    "projection {:?} has no column {c:?}",
                    def.name
                )))
            }
        };
        let mut wh = vec!["g = ?".to_string()];
        let mut binds: Vec<rusqlite::types::Value> = vec![g.into()];
        if !q.key_in.is_empty() {
            let vals: Vec<Vec<u8>> = q
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
        for (c, values) in q.filters {
            let c = column(c)?;
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
        if let Some((c, at)) = q.since {
            let c = column(c)?;
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
        let group_column = q.group_by.map(column).transpose()?;
        if matches_nothing {
            return Ok(
                json!({ "total": 0, "groups": [], "truncated": false, "tx": self.latest_tx_id()? }),
            );
        }
        let wh = wh.join(" AND ");
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
            let mut st = self.conn.prepare(&format!(
                "SELECT {c}, count(*) FROM {t} WHERE {wh} GROUP BY {c} ORDER BY count(*) DESC LIMIT {}",
                cap + 1
            ))?;
            let rows: Vec<(Option<Vec<u8>>, i64)> = st
                .query_map(rusqlite::params_from_iter(binds.iter()), |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            truncated = rows.len() > cap;
            for (v, n) in rows.into_iter().take(cap) {
                groups.push(json!({ "value": value_json(self, v)?, "n": n }));
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
