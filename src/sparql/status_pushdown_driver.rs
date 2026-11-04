//! Cost admission for a fixed-property driver of a plain-property absence scan.
use rusqlite::OptionalExtension;
use spargebra::term::NamedNodePattern;

use super::{Plan, RDF_TYPE};
use crate::{Result, Store, Value};

const PROBE_ROWS: i64 = 32_768;

/// Count a bounded prefix of the actual covering index range. This measures
/// the rows its driver visits, including lookalike attributes, without fetching
/// fact payloads or the unrelated class's status values.
fn cost(store: &Store, graph: i64, values: &[Vec<u8>]) -> Result<i64> {
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(graph)];
    let placeholders = values
        .iter()
        .map(|bytes| {
            params.push(Box::new(bytes.clone()) as Box<dyn rusqlite::types::ToSql>);
            format!("?{}", params.len())
        })
        .collect::<Vec<_>>()
        .join(",");
    store
        .prepare(&format!(
            "SELECT COUNT(*) FROM (SELECT 1 FROM facts INDEXED BY idx_active_vge
        WHERE g=?1 AND v IN ({placeholders}) AND op=1 AND valid_to IS NULL LIMIT {})",
            PROBE_ROWS + 1
        ))?
        .query_row(
            rusqlite::params_from_iter(params.iter().map(std::convert::AsRef::as_ref)),
            |r| r.get(0),
        )
        .map_err(Into::into)
}

/// Prefer a proven cheaper fixed property, keeping the same graph/current-row,
/// type and owner constraints. An absent literal owner is still evaluated; its
/// empty covering range proves there are no candidates for this arm.
pub(super) fn select(
    store: &Store,
    plan: &Plan<'_>,
    graph: i64,
    class: i64,
    params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
) -> Result<Option<String>> {
    if plan.constraints.is_empty() {
        return Ok(None);
    }
    let definition: Option<String> = store
        .conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='index' AND name='idx_active_vge'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let exact = definition.is_some_and(|sql| {
        sql.chars()
            .filter(|c| !c.is_ascii_whitespace())
            .flat_map(char::to_lowercase)
            .collect::<String>()
            == "createindexidx_active_vgeonfacts(v,g,e)whereop=1andvalid_toisnull"
    });
    if !exact {
        return Ok(None);
    }
    let mut cheapest = cost(store, graph, &[Value::Ref(class).to_bytes()])?;
    let mut selected = None;
    for constraint in &plan.constraints {
        let NamedNodePattern::NamedNode(predicate) = &constraint.predicate else {
            continue;
        };
        if predicate.as_str() == RDF_TYPE {
            continue;
        }
        let ids = store.lookup_all(predicate.as_str())?;
        let [predicate] = ids.as_slice() else {
            continue;
        };
        let Some(value) = crate::sparql::pattern_util::resolve_object_pattern(
            store,
            &constraint.object,
            &crate::sparql::Bindings::new(),
        )?
        else {
            continue;
        };
        let values = match value {
            Value::Ref(id) if id < 0 => continue,
            Value::Ref(id) => {
                if store.lookup_all(&store.resolve(id)?)?.len() != 1 {
                    continue;
                }
                vec![Value::Ref(id).to_bytes()]
            }
            other => store.literal_aliases(&other, false)?,
        };
        let measured = cost(store, graph, &values)?;
        if measured < cheapest {
            cheapest = measured;
            selected = Some((*predicate, values));
        }
    }
    let Some((predicate, values)) = selected else {
        return Ok(None);
    };
    let p = params.len() + 1;
    params.push(Box::new(predicate));
    let placeholders = values
        .into_iter()
        .map(|bytes| {
            params.push(Box::new(bytes) as Box<dyn rusqlite::types::ToSql>);
            format!("?{}", params.len())
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(Some(format!("SELECT DISTINCT t.e FROM facts AS t INDEXED BY idx_active_vge
        WHERE t.v IN ({placeholders}) AND t.g=?3 AND t.op=1 AND t.valid_to IS NULL
        AND EXISTS (SELECT 1 FROM facts AS driver INDEXED BY idx_current_aev
            WHERE driver.a=?{p} AND driver.e=t.e AND driver.rowid=t.rowid
            AND driver.op=1 AND driver.valid_to IS NULL)
        AND EXISTS (SELECT 1 FROM facts AS typed INDEXED BY idx_current_aev
            WHERE typed.a=?1 AND typed.e=t.e AND typed.v=?2 AND typed.op=1 AND typed.valid_to IS NULL
            AND EXISTS (SELECT 1 FROM facts AS tg INDEXED BY idx_current_g
                WHERE tg.g=?3 AND tg.rowid=typed.rowid AND tg.op=1 AND tg.valid_to IS NULL))")))
}
