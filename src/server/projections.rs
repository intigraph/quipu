//! `POST /projections` — read a declared current-state projection
//! (aegis-tg5 F2). Projections are installed from `[[quipu.projections]]` at
//! startup; this route only lists them and answers aggregates, so it is a READ
//! endpoint served from the read pool.
//!
//! Body:
//! - `{"action": "list"}` → the declared projections and what is installed.
//! - `{"action": "aggregate", "name": "...", "graph": "<IRI>", "key_in": [...],
//!   "where": {"<column|key>": [values...]}, "group_by": "<column|key>",
//!   "since": {"column": "...", "at": "<RFC 3339>"}, "max_groups": N}` →
//!   `{total, groups, truncated, multi_valued_rows, tx}`. Every `where` column
//!   must match, a column matches any of its values, and `null` matches an
//!   absent value.

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
                    .map(|d| json!({"name": d.name, "key": d.key, "columns": d.columns.iter().map(|c| json!({"name": c.name, "predicate": c.predicate, "time": c.time})).collect::<Vec<_>>()}))
                    .collect();
                Ok(axum::Json(json!({"declared": declared, "installed": store.installed_projections()?})))
            }
            "aggregate" => {
                let name = i.get("name").and_then(JsonValue::as_str).unwrap_or_default();
                let def = store
                    .projection_def(name)
                    .cloned()
                    .ok_or_else(|| quipu::Error::InvalidValue(format!("no declared projection {name:?}")))?;
                let key_in = i.get("key_in").and_then(JsonValue::as_array).cloned().unwrap_or_default();
                let mut filters: Vec<(String, Vec<JsonValue>)> = Vec::new();
                match i.get("where") {
                    None | Some(JsonValue::Null) => {}
                    Some(JsonValue::Object(m)) => {
                        for (c, v) in m {
                            let values = v.as_array().cloned().ok_or_else(|| {
                                quipu::Error::InvalidValue(format!(
                                    "where.{c} must be an array of values"
                                ))
                            })?;
                            filters.push((c.clone(), values));
                        }
                    }
                    Some(_) => {
                        return Err(quipu::Error::InvalidValue(
                            "where must be an object of column -> [values]".into(),
                        )
                        .into());
                    }
                }
                let filters: Vec<(&str, &[JsonValue])> =
                    filters.iter().map(|(c, v)| (c.as_str(), v.as_slice())).collect();
                let since = i.get("since");
                let since_col = since.and_then(|v| v.get("column")).and_then(JsonValue::as_str);
                let since_at = since.and_then(|v| v.get("at")).and_then(JsonValue::as_str);
                let q = quipu::ProjectionAggregate {
                    graph: i.get("graph").and_then(JsonValue::as_str).unwrap_or_default(),
                    key_in: &key_in,
                    filters: &filters,
                    group_by: i.get("group_by").and_then(JsonValue::as_str),
                    since: since_col.zip(since_at),
                    max_groups: i
                        .get("max_groups")
                        .and_then(JsonValue::as_u64)
                        .and_then(|n| usize::try_from(n).ok())
                        .unwrap_or(0),
                };
                Ok(axum::Json(store.aggregate_projection(&def, &q)?))
            }
            other => Err(quipu::Error::InvalidValue(format!(
                "unknown action {other:?}; expected list or aggregate"
            ))
            .into()),
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::extract::State;
    use serde_json::json;

    use super::projections;

    const EX: &str = "http://ex.test/";

    fn handle() -> (tempfile::TempDir, super::SharedStore) {
        let (dir, h) = super::super::tests::pooled_handle(0);
        let def = quipu::config::ProjectionConfig {
            name: "items".into(),
            key: format!("{EX}status"),
            columns: vec![
                quipu::config::ProjectionColumnConfig {
                    name: "priority".into(),
                    predicate: format!("{EX}priority"),
                    time: false,
                },
                quipu::config::ProjectionColumnConfig {
                    name: "assignee".into(),
                    predicate: format!("{EX}assignee"),
                    time: false,
                },
            ],
        };
        {
            let mut store = h.lock();
            quipu::tool_knot(
                &mut store,
                &json!({"turtle": format!(
                    "@prefix ex: <{EX}> .
                     ex:a ex:status \"open\" ; ex:priority 1 ; ex:assignee \"wu\" .
                     ex:b ex:status \"closed\" ; ex:priority 1 ; ex:assignee \"wu\" .
                     ex:c ex:status \"open\" ; ex:priority 2 ; ex:assignee \"wu\" .
                     ex:d ex:status \"open\" ; ex:priority 1 ; ex:assignee \"ian\" ."
                )}),
            )
            .unwrap();
            store.set_projection_defs(vec![def.clone()]);
            store.install_projections(&[def]).unwrap();
        }
        (dir, Arc::new(h))
    }

    #[tokio::test]
    async fn where_combines_column_filters_with_a_grouping() {
        let (_dir, s) = handle();
        // assignee = wu AND priority = 1, grouped by status (aegis-tg5 7mm).
        let a = projections(
            State(s.clone()),
            axum::Json(json!({"action": "aggregate", "name": "items",
                "where": {"assignee": ["wu"], "priority": [1]}, "group_by": "key"})),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(a["total"], 2, "{a}");
        let mut g: Vec<_> = a["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| {
                (
                    g["value"].as_str().unwrap().to_string(),
                    g["n"].as_i64().unwrap(),
                )
            })
            .collect();
        g.sort();
        assert_eq!(g, vec![("closed".into(), 1), ("open".into(), 1)]);
        for (body, needle) in [
            (
                json!({"name": "items", "where": ["assignee"]}),
                "where must be an object",
            ),
            (
                json!({"name": "items", "where": {"assignee": "wu"}}),
                "must be an array",
            ),
            (
                json!({"name": "items", "where": {"team": ["x"]}}),
                "has no column",
            ),
        ] {
            let err = projections(State(s.clone()), axum::Json(body))
                .await
                .unwrap_err();
            assert!(format!("{err:?}").contains(needle), "{err:?}");
        }
    }
}
