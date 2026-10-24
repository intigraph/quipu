//! `POST /projections` — read a declared current-state projection
//! (aegis-tg5 F2). Projections are installed from `[[quipu.projections]]` at
//! startup; this route only lists them and answers aggregates, so it is a READ
//! endpoint served from the read pool.
//!
//! Body:
//! - `{"action": "list"}` → the declared projections and what is installed.
//! - `{"action": "aggregate", "name": "...", "graph": "<IRI>", "key_in": [...],
//!   "where": {"<column|key>": [values...]}, "group_by": "<column|key>",
//!   "since": {"column": "...", "at": "<RFC 3339>"}, "until": {...},
//!   "max_groups": N}` → `{total, groups, truncated, multi_valued_rows, tx}`.
//!   Every `where` column must match, a column matches any of its values, and
//!   `null` matches an absent value. `[since, until)` is inclusive at the start
//!   and exclusive at the end. `group_by` is a dimension or a list of at most
//!   two; a dimension is a column name, `"key"`, or
//!   `{"column": "<time column>", "bucket": "day"}` (the UTC calendar day).
//!   Two dimensions answer `values` per group instead of `value`.
//! - `{"action": "rows", ..., "after": {"entity": IRI, "graph": IRI}}` resumes
//!   strictly after a truncated page's `next_after`. A page whose `tx`
//!   differs from the first page's is another snapshot: restart, never splice.

use axum::extract::State;
use serde_json::{Value as JsonValue, json};

use super::SharedStore;
use super::base::{AppError, blocking};

/// Install the declared projections at server startup. The writer installs
/// them; a replica only reads what its primary installed. A failed install does
/// not stop the server: aggregates over an uninstalled definition refuse
/// (`CannotVerify`) instead of answering stale, which is recoverable, while a
/// server that will not boot is not.
pub(crate) fn install_at_startup(
    store: &mut quipu::Store,
    config: &quipu::QuipuConfig,
    is_replica: bool,
) {
    store.set_projection_defs(config.current_projections.clone());
    if is_replica {
        return;
    }
    match store.install_projections(&config.current_projections) {
        Ok(statuses) => {
            for s in statuses {
                eprintln!("projection {}: {} ({} rows)", s.name, s.action, s.rows);
            }
        }
        Err(e) => eprintln!(
            "warning: projections NOT installed -- {e}; /projections aggregates will refuse"
        ),
    }
}

pub(crate) async fn projections(
    State(s): State<SharedStore>,
    axum::Json(i): axum::Json<JsonValue>,
) -> Result<axum::Json<JsonValue>, AppError> {
    blocking(move || {
        let store = s.read();
        match i.get("action").and_then(JsonValue::as_str).unwrap_or("aggregate") {
            "list" => {
                let declared: Vec<JsonValue> = store
                    .projection_defs_declared()
                    .iter()
                    .map(|d| json!({"name": d.name, "key": d.key, "columns": d.columns.iter().map(|c| json!({"name": c.name, "predicate": c.predicate, "time": c.time, "equals": c.equals})).collect::<Vec<_>>()}))
                    .collect();
                Ok(axum::Json(json!({
                    "actions": ["aggregate", "list", "rows"],
                    "declared": declared,
                    "installed": store.installed_projections()?,
                })))
            }
            "aggregate" => {
                let def = declared_def(&store, &i)?;
                let p = Scoped::parse(&i)?;
                let (filters, graphs) = (p.filters(), p.graphs());
                let group_by = group_by(&i)?;
                let q = quipu::ProjectionAggregate {
                    graph: p.graph,
                    graphs: &graphs,
                    key_in: &p.key_in,
                    filters: &filters,
                    group_by: &group_by,
                    since: p.since,
                    until: p.until,
                    max_groups: count(&i, "max_groups")?,
                };
                Ok(axum::Json(store.aggregate_projection(&def, &q)?))
            }
            "rows" => {
                let def = declared_def(&store, &i)?;
                let p = Scoped::parse(&i)?;
                let (filters, graphs) = (p.filters(), p.graphs());
                let columns: Option<Vec<&str>> = match i.get("columns") {
                    None | Some(JsonValue::Null) => None,
                    Some(JsonValue::Array(a)) => Some(
                        a.iter()
                            .map(|c| {
                                c.as_str().ok_or_else(|| {
                                    quipu::Error::InvalidValue("columns are column names".into())
                                })
                            })
                            .collect::<Result<_, _>>()?,
                    ),
                    Some(_) => {
                        return Err(quipu::Error::InvalidValue(
                            "columns must be an array of column names".into(),
                        )
                        .into());
                    }
                };
                let q = quipu::ProjectionRows {
                    graph: p.graph,
                    graphs: &graphs,
                    key_in: &p.key_in,
                    filters: &filters,
                    since: p.since,
                    until: p.until,
                    after: rows_after(&i)?,
                    columns: columns.as_deref(),
                    limit: rows_limit(&i)?,
                };
                Ok(axum::Json(store.projection_rows(&def, &q)?))
            }
            other => Err(quipu::Error::InvalidValue(format!(
                "unknown action {other:?}; expected list, aggregate or rows"
            ))
            .into()),
        }
    })
    .await
}

fn declared_def(
    store: &quipu::Store,
    i: &JsonValue,
) -> Result<quipu::config::ProjectionConfig, quipu::Error> {
    let name = i
        .get("name")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    store
        .projection_def(name)
        .cloned()
        .ok_or_else(|| quipu::Error::InvalidValue(format!("no declared projection {name:?}")))
}

/// A non-negative integer field, absent = 0; anything else refuses.
fn count(i: &JsonValue, field: &str) -> Result<usize, quipu::Error> {
    match i.get(field) {
        None | Some(JsonValue::Null) => Ok(0),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| {
                quipu::Error::InvalidValue(format!("{field} must be a non-negative integer"))
            }),
    }
}

/// A graph IRI field: a string that parses as an absolute IRI.
fn graph_iri<'a>(v: &'a JsonValue, field: &str) -> Result<&'a str, quipu::Error> {
    v.as_str()
        .filter(|g| oxrdf::NamedNode::new(*g).is_ok())
        .ok_or_else(|| quipu::Error::InvalidValue(format!("{field} must be graph IRIs")))
}

/// The rows `limit`: absent = the maximum, otherwise `1..=PROJECTION_ROWS_MAX`.
fn rows_limit(i: &JsonValue) -> Result<usize, quipu::Error> {
    match i.get("limit") {
        None | Some(JsonValue::Null) => Ok(0),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=quipu::PROJECTION_ROWS_MAX).contains(n))
            .ok_or_else(|| {
                quipu::Error::InvalidValue(format!(
                    "limit must be an integer from 1 to {}",
                    quipu::PROJECTION_ROWS_MAX
                ))
            }),
    }
}

/// `group_by`: absent, one dimension, or a list of at most two (the store
/// enforces the count and checks every name against the declaration).
fn group_by(i: &JsonValue) -> Result<Vec<quipu::GroupBy<'_>>, quipu::Error> {
    fn dim(v: &JsonValue) -> Result<quipu::GroupBy<'_>, quipu::Error> {
        if let Some(c) = v.as_str() {
            return Ok(quipu::GroupBy::Column(c));
        }
        match (
            v.get("column").and_then(JsonValue::as_str),
            v.get("bucket").and_then(JsonValue::as_str),
        ) {
            (Some(c), Some("day")) if v.as_object().is_some_and(|o| o.len() == 2) => {
                Ok(quipu::GroupBy::Day(c))
            }
            _ => Err(quipu::Error::InvalidValue(
                "a group_by dimension is a column name or {\"column\": \"...\", \"bucket\": \"day\"}"
                    .into(),
            )),
        }
    }
    match i.get("group_by") {
        None | Some(JsonValue::Null) => Ok(Vec::new()),
        Some(JsonValue::Array(a)) if !a.is_empty() => a.iter().map(dim).collect(),
        Some(JsonValue::Array(_)) => Err(quipu::Error::InvalidValue(
            "group_by must not be an empty list".into(),
        )),
        Some(v) => Ok(vec![dim(v)?]),
    }
}

/// A `{"column": "...", "at": "<RFC 3339>"}` bound (`since` or `until`).
fn time_bound<'a>(
    i: &'a JsonValue,
    field: &str,
) -> Result<Option<(&'a str, &'a str)>, quipu::Error> {
    match i.get(field) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(v) => {
            let part = |f: &str| v.get(f).and_then(JsonValue::as_str);
            Ok(Some(part("column").zip(part("at")).ok_or_else(|| {
                quipu::Error::InvalidValue(format!(
                    "{field} must be {{\"column\": \"...\", \"at\": \"<RFC 3339>\"}}"
                ))
            })?))
        }
    }
}

/// The rows cursor: a previous page's `next_after`, verbatim.
fn rows_after(i: &JsonValue) -> Result<Option<quipu::RowsAfter<'_>>, quipu::Error> {
    match i.get("after") {
        None | Some(JsonValue::Null) => Ok(None),
        Some(v) => {
            let part = |f: &str| v.get(f).and_then(JsonValue::as_str);
            let (entity, graph) = part("entity").zip(part("graph")).ok_or_else(|| {
                quipu::Error::InvalidValue(
                    "after must be a page's next_after: {\"entity\": IRI, \"graph\": IRI}".into(),
                )
            })?;
            Ok(Some(quipu::RowsAfter { entity, graph }))
        }
    }
}

/// The scoping fields aggregates and row listings share, parsed once.
struct Scoped<'a> {
    graph: &'a str,
    graphs: Vec<&'a str>,
    key_in: Vec<JsonValue>,
    filters: Vec<(String, Vec<JsonValue>)>,
    since: Option<(&'a str, &'a str)>,
    until: Option<(&'a str, &'a str)>,
}

impl<'a> Scoped<'a> {
    fn parse(i: &'a JsonValue) -> Result<Self, quipu::Error> {
        let mut filters = Vec::new();
        match i.get("where") {
            None | Some(JsonValue::Null) => {}
            Some(JsonValue::Object(m)) => {
                for (c, v) in m {
                    let values = v.as_array().cloned().ok_or_else(|| {
                        quipu::Error::InvalidValue(format!("where.{c} must be an array of values"))
                    })?;
                    filters.push((c.clone(), values));
                }
            }
            Some(_) => {
                return Err(quipu::Error::InvalidValue(
                    "where must be an object of column -> [values]".into(),
                ));
            }
        }
        let graphs = match i.get("graphs") {
            None | Some(JsonValue::Null) => Vec::new(),
            Some(JsonValue::Array(a)) if !a.is_empty() => a
                .iter()
                .map(|g| graph_iri(g, "graphs entries"))
                .collect::<Result<_, _>>()?,
            Some(_) => {
                return Err(quipu::Error::InvalidValue(
                    "graphs must be a non-empty array of graph IRIs".into(),
                ));
            }
        };
        // An explicit field that is malformed refuses; it never falls back to
        // the default an omitted field gets.
        let graph = match i.get("graph") {
            None | Some(JsonValue::Null) => "",
            Some(g) => graph_iri(g, "graph")?,
        };
        let key_in = match i.get("key_in") {
            None | Some(JsonValue::Null) => Vec::new(),
            Some(JsonValue::Array(a)) => a.clone(),
            Some(_) => {
                return Err(quipu::Error::InvalidValue(
                    "key_in must be an array of key values".into(),
                ));
            }
        };
        Ok(Self {
            graph,
            graphs,
            key_in,
            filters,
            since: time_bound(i, "since")?,
            until: time_bound(i, "until")?,
        })
    }

    fn filters(&self) -> Vec<(&str, &[JsonValue])> {
        self.filters
            .iter()
            .map(|(c, v)| (c.as_str(), v.as_slice()))
            .collect()
    }

    fn graphs(&self) -> Vec<&str> {
        self.graphs.clone()
    }
}

#[cfg(test)]
#[path = "projections_tests.rs"]
mod tests;
