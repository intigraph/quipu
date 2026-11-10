use super::*;
use crate::merge_core::{Card, FactOp, SlotInput, plan_slot};
use crate::{Datum, Op};
use std::collections::BTreeMap;

fn values(store: &Store, facts: &[crate::Fact]) -> Result<BTreeMap<String, String>> {
    facts
        .iter()
        .map(|f| {
            Ok((
                crate::rdf::value_to_term(store, &f.value)?.to_string(),
                f.valid_from.clone(),
            ))
        })
        .collect()
}

fn lower(
    store: &Store,
    slot: &Slot,
    op: FactOp<String>,
    current: &BTreeMap<String, String>,
) -> Result<Datum> {
    let (value, from, to, op) = match op {
        FactOp::Assert { value, from } => (value, from, None, Op::Assert),
        FactOp::Receipt { value, at } => (value, at.clone(), Some(at), Op::Assert),
        FactOp::Close { value, at } => {
            let start = current
                .get(&value)
                .ok_or_else(|| refused("close provenance"))?
                .clone();
            (value, start, Some(at), Op::Retract)
        }
    };
    let subject = slot
        .subject
        .parse::<oxrdf::Term>()
        .map_err(|_| refused("subject"))?;
    let oxrdf::Term::NamedNode(subject) = subject else {
        return Err(refused("named subject required"));
    };
    let term = value
        .parse::<oxrdf::Term>()
        .map_err(|_| refused("incoming term"))?;
    if term.to_string() != value || matches!(term, oxrdf::Term::BlankNode(_)) {
        return Err(refused("canonical stable incoming term required"));
    }
    Ok(Datum {
        entity: store.intern(subject.as_str())?,
        attribute: store.intern(&slot.predicate)?,
        value: crate::rdf::term_to_value(store, &term)?,
        valid_from: from,
        valid_to: to,
        op,
    })
}

pub(super) fn apply(store: &mut Store, request: &Proposal, batch: &BatchViews) -> Result<i64> {
    let mut receive = Vec::new();
    let mut decide = Vec::new();
    for (view, incoming) in batch.slots.iter().zip(&request.incoming) {
        // This profile is deliberately EXACT, not an arbitrary shape parser.
        if view.slot.predicate != "urn:p" {
            return Err(refused("fixture profile scope"));
        }
        let base = values(store, &view.base)?;
        let current = values(store, &view.current)?;
        let theirs: BTreeMap<_, _> = incoming
            .iter()
            .map(|v| (v.clone(), request.valid_at.clone()))
            .collect();
        if theirs.len() != incoming.len() {
            return Err(refused("duplicate incoming term"));
        }
        let input = SlotInput {
            base: base.keys().cloned().collect(),
            ours: current.clone(),
            theirs: theirs.clone(),
            theirs_closed: base
                .keys()
                .filter(|v| !theirs.contains_key(*v))
                .map(|v| (v.clone(), request.valid_at.clone()))
                .collect(),
            card: Card::ONE,
        };
        let plan = plan_slot(&input, None, &request.valid_at)?;
        if plan.held {
            return Err(refused("unresolved conflict; pending ledger not wired"));
        }
        for op in plan.receive {
            receive.push(lower(store, &view.slot, op, &current)?);
        }
        for op in plan.decide {
            decide.push(lower(store, &view.slot, op, &current)?);
        }
    }
    let mut tx = 0;
    for datums in [receive, decide] {
        if datums.is_empty() {
            continue;
        }
        let (result, observations) = store.observe_validator(|s| {
            s.transact(
                &datums,
                &request.valid_at,
                None,
                Some("reserved admission fixture"),
            )
        });
        tx = result?;
        validate(store, request, tx, &observations)?;
    }
    if tx == 0 {
        return Err(refused("no data; settlement contract not wired"));
    }
    Ok(tx)
}

fn validate(
    store: &Store,
    request: &Proposal,
    tx: i64,
    observations: &[super::super::ValidatorObservation],
) -> Result<()> {
    if observations.len() != 1 {
        return Err(refused("one actual validator observation required"));
    }
    let o = &observations[0];
    let m = o.manifest()?;
    m.identity()?;
    let c = o
        .coordinates()
        .ok_or_else(|| refused("validator coordinates"))?;
    // Same ordered predicates and refusal behavior; name the FIRST failed
    // binding without printing graph bytes or silently weakening a guard.
    binding(
        c.transaction == u64::try_from(tx).map_err(|_| refused("transaction range"))?,
        "transaction",
    )?;
    binding(c.accepted_at == request.valid_at, "accepted_at")?;
    binding(o.graph() == crate::schema::ROOT_GRAPH, "graph")?;
    binding(o.gate_error().is_none(), "gate_error")?;
    binding(profile(store)? == request.profile, "profile")?;
    binding(m.combined_shapes == SHAPES.as_bytes(), "combined_shapes")?;
    binding(m.loaded_ontologies.is_empty(), "loaded_ontologies")?;
    binding(m.flags.shacl_config, "shacl_config")?;
    binding(!m.flags.owl_validate_on_write, "owl_validate_on_write")?;
    binding(!m.flags.recording_verdicts, "recording_verdicts")?;
    binding(
        !m.flags.postcommit_reactive_materialize,
        "postcommit_reactive_materialize",
    )?;
    binding(m.incremental.write_shapes.is_none(), "write_shapes")?;
    // Bound fixture whole-graph audit, deliberately not a production cost claim.
    let facts=store.post_state_facts("SELECT e,a,v,tx,valid_from,valid_to FROM facts WHERE g=?1 AND op=1 AND valid_to IS NULL ORDER BY e,a,v,tx LIMIT 4097",&[crate::schema::ROOT_GRAPH])?;
    if facts.len() > 4096 {
        return Err(refused("staged fixture budget"));
    }
    let staged = store.cardinality_context(&facts)?;
    if staged.as_bytes() != m.staged_graph {
        return Err(refused("staged bytes moved"));
    }
    full_conformance(&staged)
}

fn binding(matches: bool, predicate: &str) -> Result<()> {
    if matches {
        Ok(())
    } else {
        Err(refused(&format!("actual validator binding: {predicate}")))
    }
}

#[cfg(feature = "shacl")]
fn full_conformance(staged: &str) -> Result<()> {
    if crate::validate_shapes(SHAPES, staged)?.conforms {
        Ok(())
    } else {
        Err(refused("full staged graph violates shapes"))
    }
}
#[cfg(not(feature = "shacl"))]
fn full_conformance(_staged: &str) -> Result<()> {
    Err(refused("SHACL unavailable"))
}
