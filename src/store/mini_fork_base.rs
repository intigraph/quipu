//! Base journal and digest for a mini-Quipu fork registry entry (M1).
//! Reuses the local-sync `BaseJournal` rows and `base_slice_hash` form.
use super::invalid;
use crate::{
    Error, Result, Store,
    changeset::Statement,
    client_changeset::{BaseJournal, BaseRow, digest},
    schema::ROOT_GRAPH_IRI,
};

const MAX_ROWS: usize = 10_000;

/// One member's frozen base at `cut`, in the local-sync journal form.
/// Bounded intervals that were already bounded at the cut refuse rather than
/// being flattened, exactly as the master base journal does.
pub fn member_journal(store: &Store, cut: i64, graph: &str) -> Result<BaseJournal> {
    let g = if graph == ROOT_GRAPH_IRI {
        0
    } else {
        store
            .committed_graph_id(graph)?
            .ok_or_else(|| invalid("member graph is absent from the home owner"))?
    };
    if store.fact_count_as_of(g, cut)? > MAX_ROWS {
        return Err(invalid("base row budget exceeded"));
    }
    let as_of = crate::store::AsOf {
        tx: Some(cut),
        valid_at: None,
    };
    let mut rows = Vec::new();
    for fact in store.facts_as_of_in_graph(&as_of, g)? {
        if fact.valid_to.is_some() {
            let closed_after = store
                .conn
                .prepare(
                    "SELECT 1 FROM facts WHERE g=?1 AND e=?2 AND a=?3 AND v=?4 AND tx=?5 \
                     AND valid_from=?6 AND retracted_tx>?7 LIMIT 1",
                )?
                .exists(rusqlite::params![
                    g,
                    fact.entity,
                    fact.attribute,
                    fact.value.to_bytes(),
                    fact.tx,
                    fact.valid_from,
                    cut
                ])?;
            if !closed_after {
                return Err(invalid("bounded base intervals unsupported"));
            }
        }
        rows.push(BaseRow {
            triple: crate::changeset_store::fact_statement(store, &fact, None)?.triple,
            valid_from: fact.valid_from,
            valid_to: None,
        });
    }
    BaseJournal::new(
        store.store_id()?,
        store.log_epoch()?,
        cut,
        if graph == ROOT_GRAPH_IRI {
            "ROOT"
        } else {
            graph
        }
        .into(),
        shapes_hash(store, cut)?,
        rows,
    )
}

pub(super) fn shapes_hash(store: &Store, cut: i64) -> Result<String> {
    let shapes = store
        .get_combined_shapes_as_of(&crate::store::AsOf {
            tx: Some(cut),
            valid_at: None,
        })?
        .unwrap_or_default();
    Ok(digest(shapes.as_bytes()))
}

/// The local-sync `base_slice_hash` construction over a set of journals:
/// each canonical N-Quad line paired with its valid interval, sorted, then
/// JSON-encoded and hashed. For one journal this equals its `slice_hash`.
pub fn base_digest(journals: &[BaseJournal]) -> Result<String> {
    let mut lines = Vec::new();
    for journal in journals {
        let graph = (journal.graph() != "ROOT").then(|| journal.graph().to_owned());
        for row in journal.rows() {
            lines.push((
                format!("{} .", Statement::new(&row.triple, graph.clone()).nquad()),
                &row.valid_from,
                &row.valid_to,
            ));
        }
    }
    lines.sort();
    Ok(digest(
        &serde_json::to_vec(&lines).map_err(|e| Error::Serialization(e.to_string()))?,
    ))
}

/// Digest of a store's LIVE writable members as of `cut`: exactly the rows
/// the merge planner reads as a base (`graph_facts_as_of`), in the journal
/// line form of [`base_digest`]. A later merge back plans against the
/// mini-Quipu's own state at the merged head; home records this digest at
/// merge time, and a rewritten or compacted history fails it.
pub fn live_digest(store: &Store, cut: i64, writable: &[String]) -> Result<String> {
    let mut journals = Vec::new();
    for graph in writable {
        let g = if graph == ROOT_GRAPH_IRI {
            0
        } else {
            store
                .committed_graph_id(graph)?
                .ok_or_else(|| invalid("member graph is absent"))?
        };
        let facts = store.graph_facts_as_of(g, cut)?;
        if facts.len() > MAX_ROWS {
            return Err(invalid("base row budget exceeded"));
        }
        let rows = facts
            .iter()
            .map(|fact| {
                Ok(BaseRow {
                    triple: crate::changeset_store::fact_statement(store, fact, None)?.triple,
                    valid_from: fact.valid_from.clone(),
                    valid_to: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        journals.push(BaseJournal::new(
            store.store_id()?,
            store.log_epoch()?,
            cut,
            if graph == ROOT_GRAPH_IRI {
                "ROOT"
            } else {
                graph
            }
            .into(),
            shapes_hash(store, cut)?,
            rows,
        )?);
    }
    base_digest(&journals)
}
