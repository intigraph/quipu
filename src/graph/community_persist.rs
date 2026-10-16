//! One atomic, producer-owned derived snapshot; prior values remain historical.
use super::communities::{NamedCommunity, previous};
use super::community_identity::digest;
use crate::store::Datum;
use crate::types::{Op, Value};
use crate::{Error, Result, Store, namespace};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
const SOURCE: &str = "algo:louvain-communities/v1";
const PROV: &str = "http://www.w3.org/ns/prov#";

fn datum(store: &Store, e: &str, a: &str, value: Value, timestamp: &str) -> Result<Datum> {
    Ok(Datum {
        entity: store.intern(e)?,
        attribute: store.intern(a)?,
        value,
        valid_from: timestamp.into(),
        valid_to: None,
        op: Op::Assert,
    })
}
fn reference(store: &Store, iri: &str) -> Result<Value> {
    Ok(Value::Ref(store.intern(iri)?))
}

pub(super) fn persist(
    store: &mut Store,
    graph: &str,
    groups: &[NamedCommunity],
    input: &Json,
    modularity: f64,
    timestamp: &str,
) -> Result<()> {
    // The graph is reserved to this producer. Never reconcile user/other producer
    // assertions merely because they use one of our predicates.
    if let Some(g) = store.registered_graph_id(graph)? {
        let foreign: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM facts f JOIN transactions t ON t.id=f.tx WHERE f.g=?1 AND f.op=1 AND f.valid_to IS NULL AND (t.source IS NULL OR t.source<>?2)",
            rusqlite::params![g,SOURCE], |r|r.get(0))?;
        if foreign != 0 {
            return Err(Error::InvalidValue(
                "derived community graph contains foreign assertions".into(),
            ));
        }
    }
    let old_groups = previous(store, graph)?;
    let source_tx = store.latest_tx_id()?;
    let params = json!({"algorithm":"louvain/v1", "graph":input.get("graph"),"type":input.get("type"),"predicate":input.get("predicate"),"match_threshold":input.get("match_threshold").cloned().unwrap_or(json!(0.5))});
    let run = format!(
        "urn:quipu:grouping-run:{}",
        digest(&[
            graph.into(),
            timestamp.into(),
            source_tx.to_string(),
            params.to_string()
        ])
    );
    store.conn.execute_batch("SAVEPOINT quipu_community_run")?;
    let result = (|| {
        let existed = store.registered_graph_id(graph)?.is_some();
        let g = store.graph_create(graph)?;
        if !existed {
            store.set_graph_label(
                graph,
                &crate::store::labels::GraphLabel {
                    kind: Some(crate::lattice_kind::DataKind::parse("derived")?),
                    ..Default::default()
                },
                timestamp,
                None,
            )?;
        }
        let mut wanted = Vec::new();
        let quipu = namespace::QUIPU;
        for group in groups {
            let id = &group.community;
            wanted.push(datum(
                store,
                id,
                namespace::RDF_TYPE,
                reference(store, &format!("{quipu}Group"))?,
                timestamp,
            )?);
            wanted.push(datum(
                store,
                id,
                &format!("{}label", namespace::RDFS),
                Value::Str(group.label.clone()),
                timestamp,
            )?);
            wanted.push(datum(
                store,
                id,
                &format!("{quipu}groupKind"),
                Value::Str("community".into()),
                timestamp,
            )?);
            wanted.push(datum(
                store,
                id,
                &format!("{quipu}size"),
                Value::Int(group.size as i64),
                timestamp,
            )?);
            wanted.push(datum(
                store,
                id,
                &format!("{quipu}labelBasis"),
                Value::Str(group.label_basis.to_string()),
                timestamp,
            )?);
            wanted.push(datum(
                store,
                id,
                &format!("{PROV}wasGeneratedBy"),
                reference(store, &run)?,
                timestamp,
            )?);
            for member in &group.entities {
                wanted.push(datum(
                    store,
                    member,
                    &format!("{quipu}memberOfCommunity"),
                    reference(store, id)?,
                    timestamp,
                )?);
            }
            for parent in &group.derived_from {
                wanted.push(datum(
                    store,
                    id,
                    &format!("{PROV}wasDerivedFrom"),
                    reference(store, parent)?,
                    timestamp,
                )?);
            }
        }
        // A disappeared old group with exactly one descendant was absorbed.
        let active: BTreeSet<_> = groups.iter().map(|g| g.community.as_str()).collect();
        for old in old_groups
            .keys()
            .filter(|old| !active.contains(old.as_str()))
        {
            let children: Vec<_> = groups
                .iter()
                .filter(|g| g.derived_from.contains(old))
                .collect();
            if children.len() == 1 {
                wanted.push(datum(
                    store,
                    old,
                    &format!("{quipu}mergedInto"),
                    reference(store, &children[0].community)?,
                    timestamp,
                )?);
            }
        }
        wanted.push(datum(
            store,
            &run,
            namespace::RDF_TYPE,
            reference(store, &format!("{quipu}GroupingRun"))?,
            timestamp,
        )?);
        wanted.push(datum(
            store,
            &run,
            &format!("{quipu}parameters"),
            Value::Str(params.to_string()),
            timestamp,
        )?);
        wanted.push(datum(
            store,
            &run,
            &format!("{quipu}sourceTx"),
            Value::Int(source_tx),
            timestamp,
        )?);
        wanted.push(datum(
            store,
            &run,
            &format!("{quipu}modularity"),
            Value::Float(modularity),
            timestamp,
        )?);
        wanted.push(datum(
            store,
            &run,
            &format!("{PROV}generatedAtTime"),
            Value::Str(timestamp.into()),
            timestamp,
        )?);
        // Reconcile this graph only. The transaction's bitemporal retracts retain
        // all old memberships, labels and run provenance for as-of readers.
        let mut desired: BTreeMap<_, _> = wanted
            .into_iter()
            .map(|d| ((d.entity, d.attribute, d.value.term_key()), d))
            .collect();
        let mut changes = Vec::new();
        for fact in store.current_facts_in_graph(g)? {
            if desired
                .remove(&(fact.entity, fact.attribute, fact.value.term_key()))
                .is_none()
            {
                changes.push(Datum {
                    entity: fact.entity,
                    attribute: fact.attribute,
                    value: fact.value,
                    valid_from: timestamp.into(),
                    valid_to: None,
                    op: Op::Retract,
                });
            }
        }
        changes.extend(desired.into_values());
        store.transact_to_graph(&changes, timestamp, None, Some(SOURCE), g)?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            store.conn.execute_batch("RELEASE quipu_community_run")?;
            Ok(())
        }
        Err(e) => {
            let _ = store
                .conn
                .execute_batch("ROLLBACK TO quipu_community_run; RELEASE quipu_community_run");
            store.term_cache.borrow_mut().clear_persistent();
            Err(e)
        }
    }
}
