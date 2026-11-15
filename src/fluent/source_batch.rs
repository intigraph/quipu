//! Bounded union admission inside one existing normal writer transaction.
use super::*;
use crate::fluent::writer::Prepared;
use crate::store::Datum;
use std::collections::BTreeMap;

const MAX_EVENTS: usize = 64;

pub(crate) fn materialize_batch(
    proofs: &[Proof],
    store: &Store,
    graph: i64,
    at: &str,
) -> Result<Prepared> {
    if proofs.is_empty() || proofs.len() > MAX_EVENTS {
        return Err(invalid("source event batch is empty or exceeds bound"));
    }
    let (_, mut log) = native_registry::load(store, graph)?;
    let mut events = BTreeMap::new();
    let mut policies = BTreeMap::new();
    let mut datums: BTreeMap<(i64, i64), Datum> = BTreeMap::new();
    let mut replay = None;
    for proof in proofs {
        // Every proof rechecks actual receiver identity and current governed
        // authority here, under the same SQLite writer reservation.
        let prepared = proof.materialize(store, graph, at)?;
        let observation = &proof.body.observation;
        let bytes = serde_json::to_vec(proof).map_err(|_| invalid("source proof encoding"))?;
        if let Some(prior) = events.insert(observation.order.event.clone(), bytes.clone())
            && prior != bytes
        {
            return Err(invalid("source batch reuses event with a different proof"));
        }
        if let Some(prior) = policies.insert(observation.fluent.clone(), proof.body.slot.policy)
            && prior != proof.body.slot.policy
        {
            return Err(invalid("source batch rebinds fluent policy"));
        }
        log.append(observation.clone())?;
        match prepared {
            Prepared::Replay(tx) => replay = Some(replay.map_or(tx, |prior: i64| prior.min(tx))),
            Prepared::Write(rows) => {
                for datum in rows {
                    let key = (datum.entity, datum.attribute);
                    if let Some(prior) = datums.get(&key) {
                        if prior.value != datum.value
                            || prior.op != datum.op
                            || prior.valid_from != datum.valid_from
                            || prior.valid_to != datum.valid_to
                        {
                            return Err(invalid(
                                "source batch has conflicting native record fields",
                            ));
                        }
                    } else {
                        datums.insert(key, datum);
                    }
                }
            }
        }
    }
    // Individually valid deltas can overflow only when combined. Derive the
    // complete union before returning any rows to the normal staging gates.
    for (fluent, policy) in policies {
        log.current(&fluent, policy)?;
    }
    if datums.is_empty() {
        return Ok(Prepared::Replay(
            replay.ok_or_else(|| invalid("empty source batch result"))?,
        ));
    }
    Ok(Prepared::Write(datums.into_values().collect()))
}

pub(crate) fn persist_batch(proofs: &[Proof], store: &Store, graph: i64, tx: i64) -> Result<()> {
    for proof in proofs {
        let event = store
            .lookup(&proof.body.observation.order.event)?
            .ok_or_else(|| invalid("missing admitted source batch event"))?;
        // Existing and in-packet duplicates were compared byte-for-byte during
        // materialization. Each newly staged event retains exactly one receipt.
        if store.fluent_origin_receipt(graph, event)?.is_none() {
            proof.persist(store, graph, tx)?;
        }
    }
    Ok(())
}
