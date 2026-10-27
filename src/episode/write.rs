//! One atomic episode write, including superseded activity metadata.

use super::{Episode, descriptions, sanitize_iri_local};
use crate::{Datum, Op, Result, Store};

#[allow(clippy::too_many_arguments)]
pub(super) fn commit(
    store: &mut Store,
    episode: &Episode,
    turtle: &str,
    timestamp: &str,
    base_ns: &str,
    actor: Option<&str>,
    graph: i64,
) -> Result<(i64, usize)> {
    store.with_write_shapes(episode.shapes.as_deref(), |store| {
        let mut assertions = crate::rdf::parse_rdf(
            store,
            turtle.as_bytes(),
            oxrdfio::RdfFormat::Turtle,
            None,
            timestamp,
        )?;
        let assertion_count = assertions.len();
        let stale = if episode.replace_snapshot {
            store.plan_episode_replacement(&episode.name, graph, &assertions)?
        } else if let Some(ep) = store.lookup(&format!(
            "{base_ns}episode_{}",
            sanitize_iri_local(&episode.name)
        ))? {
            store
                .entity_facts_in_graph(ep, graph)?
                .into_iter()
                .map(|f| Datum {
                    entity: f.entity,
                    attribute: f.attribute,
                    value: f.value,
                    valid_from: f.valid_from,
                    valid_to: None,
                    op: Op::Retract,
                })
                .collect()
        } else {
            Vec::new()
        };
        // Keep unchanged statements active: one tx cannot both retract and
        // assert the same term. Changed metadata must roll back along with a
        // refused payload, rather than committing a separate early retraction.
        let keys: std::collections::BTreeSet<_> = assertions
            .iter()
            .map(|d| (d.entity, d.attribute, d.value.term_key()))
            .collect();
        let mut datums: Vec<_> = stale
            .into_iter()
            .filter(|d| !keys.contains(&(d.entity, d.attribute, d.value.term_key())))
            .collect();
        datums.append(&mut assertions);
        if !episode.replace_snapshot {
            descriptions::reconcile_node_descriptions(store, episode, base_ns, graph, &mut datums)?;
            descriptions::keep_existing_node_labels(store, graph, &mut datums)?;
        }
        let count = if episode.replace_snapshot {
            assertion_count
        } else {
            datums.len()
        };
        let source = format!("episode:{}", episode.name);
        let tx = if episode.replace_snapshot {
            store.transact_snapshot(&datums, timestamp, actor, &source, graph)?
        } else {
            store.transact_to_graph(&datums, timestamp, actor, Some(&source), graph)?
        };
        Ok((tx, count))
    })
}
