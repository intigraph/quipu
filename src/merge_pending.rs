//! Durable partial-merge receipts, using normal named-graph facts and gates.
//! The ledger graph is an explicit caller admission, never ROOT or an inferred
//! private graph. No command installs or invokes this source-stage API yet.
use crate::error::{Error, Result};
use crate::merge_core::FactOp;
use crate::merge_core::selection::Mode;
use crate::merge_lists::Slot;
use crate::merge_session::{self, Prepared, Request};
use crate::store::{Datum, Store};
use crate::types::{Op, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const PAYLOAD: &str = "urn:quipu:merge-session:receipt";
const MAX_RECORD: usize = 8 * 1024 * 1024;

#[path = "merge_pending_report.rs"]
mod report;
pub(crate) use report::ReportRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Guard {
    pub slot: Slot,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub schema: String,
    pub id: String,
    pub request: Request,
    pub authored_at: String,
    pub receive_tx: Option<i64>,
    pub decide_tx: Option<i64>,
    pub applied: usize,
    pub held: usize,
    pub rows: Vec<ReportRow>,
    pub held_units: Vec<BTreeSet<Slot>>,
    pub guards: Vec<Guard>,
    pub store_shapes_hash: String,
    pub reviewed_choices: Vec<crate::merge_session::Choice>,
}

fn invalid(message: &str) -> Error {
    Error::InvalidValue(format!("partial merge: {message}"))
}
fn entity(id: &str) -> String {
    format!("urn:quipu:merge-session:{id}")
}
fn subject_id(store: &Store, subject: &str) -> Result<Option<i64>> {
    if !subject.starts_with('<') || !subject.ends_with('>') {
        return Err(invalid("stable IRI subject required for store replay"));
    }
    store.lookup(&subject[1..subject.len() - 1])
}

fn guard(store: &Store, slot: &Slot, request: &Request) -> Result<String> {
    let mut subjects = BTreeSet::from([slot.0.clone()]);
    if request
        .profile
        .lists
        .iter()
        .any(|binding| binding.slot == *slot)
    {
        let (root, _) = crate::share_merge::root_graph(store)?;
        let declarations = request
            .profile
            .lists
            .iter()
            .map(|b| (b.slot.clone(), b.mode))
            .collect();
        if let Some(value) = crate::merge_lists::read(&root, &declarations)?
            .get(slot)
            .and_then(Option::as_ref)
        {
            subjects.extend(value.cells.iter().cloned());
        }
    }
    let mut rows = Vec::new();
    for subject in subjects {
        if let Some(id) = subject_id(store, &subject)? {
            for fact in store.entity_facts(id)? {
                rows.push((
                    subject.clone(),
                    fact.attribute,
                    fact.value.term_key(),
                    fact.tx,
                    fact.valid_from,
                    fact.valid_to,
                ));
            }
        }
    }
    rows.sort();
    Ok(crate::share::sha256(
        &serde_json::to_vec(&rows).map_err(|e| Error::Serialization(e.to_string()))?,
    ))
}

pub(crate) fn load(store: &Store, ledger: &str, id: &str) -> Result<Option<Receipt>> {
    let (Some(graph), Some(subject), Some(attribute)) = (
        store.lookup(ledger)?,
        store.lookup(&entity(id))?,
        store.lookup(PAYLOAD)?,
    ) else {
        return Ok(None);
    };
    let facts: Vec<_> = store
        .entity_facts_in_graph(subject, graph)?
        .into_iter()
        .filter(|f| f.attribute == attribute)
        .collect();
    if facts.len() > 1 {
        return Err(invalid("ambiguous stored receipt"));
    }
    let Some(fact) = facts.first() else {
        return Ok(None);
    };
    // A ledger can retain private incoming snapshots. Possessing its IRI or
    // guessing a receipt id is not read authority. Unattributed local callers
    // can resume only unattributed local receipts; HTTP requires named identity.
    match (
        crate::transaction_auth::current_identity(),
        store.transaction_auth(fact.tx)?,
    ) {
        (None, None) => {}
        (Some(current), Some(recorded))
            if current.principal == recorded.principal
                && matches!(
                    current.auth_class.as_str(),
                    "named_bearer" | "attested_session"
                )
                && matches!(
                    recorded.auth_class.as_str(),
                    "named_bearer" | "attested_session"
                ) => {}
        _ => {
            return Err(Error::PolicyDenied(
                "merge receipt belongs to another authenticated writer".into(),
            ));
        }
    }
    let Value::Str(payload) = &fact.value else {
        return Err(invalid("receipt is not JSON text"));
    };
    if payload.len() > MAX_RECORD {
        return Err(invalid("receipt exceeds bound"));
    }
    let receipt: Receipt =
        serde_json::from_str(payload).map_err(|e| invalid(&format!("invalid receipt: {e}")))?;
    if receipt.schema != "quipu-partial-merge/v1"
        || receipt.id != id
        || receipt.request.store_id != store.store_id()?
    {
        return Err(invalid("foreign or mismatched receipt"));
    }
    let expected = crate::share::sha256(
        &serde_json::to_vec(&(
            "quipu-merge-session/v1",
            &receipt.request,
            &receipt.reviewed_choices,
            &receipt.authored_at,
        ))
        .map_err(|e| Error::Serialization(e.to_string()))?,
    );
    if expected != receipt.id {
        return Err(invalid("receipt request/decision digest differs"));
    }
    let transaction = store
        .get_transaction(fact.tx)?
        .ok_or_else(|| invalid("missing receipt transaction"))?;
    if transaction.source.as_deref()
        != Some(format!("merge-session;merge_id={id};step=record").as_str())
    {
        return Err(invalid("receipt is not bound to its writer transaction"));
    }
    let parent = receipt
        .request
        .parent
        .as_ref()
        .map_or(String::new(), |id| format!("resume_parent={id};"));
    for (step, tx) in [
        ("receive", receipt.receive_tx),
        ("decide", receipt.decide_tx),
    ] {
        if let Some(tx) = tx {
            let actual = store
                .get_transaction(tx)?
                .ok_or_else(|| invalid("missing sequenced transaction"))?;
            if actual.source.as_deref()
                != Some(format!("merge-session;merge_id={id};{parent}step={step}").as_str())
            {
                return Err(invalid("sequenced transaction identity differs"));
            }
        }
    }
    Ok(Some(receipt))
}

fn datums(store: &mut Store, ops: &[FactOp<oxrdf::Triple>]) -> Result<Vec<Datum>> {
    let mut result = Vec::new();
    for operation in ops {
        let (triple, from, to, op) = match operation {
            FactOp::Assert { value, from } => (value, from.clone(), None, Op::Assert),
            FactOp::Receipt { value, at } => (value, at.clone(), Some(at.clone()), Op::Assert),
            FactOp::Close { value, at } => {
                let e = subject_id(store, &value.subject.to_string())?
                    .ok_or_else(|| invalid("missing ROOT subject"))?;
                let a = store
                    .lookup(value.predicate.as_str())?
                    .ok_or_else(|| invalid("missing ROOT predicate"))?;
                let mut starts = Vec::new();
                for fact in store.entity_facts(e)? {
                    if fact.attribute == a
                        && crate::rdf::value_to_term(store, &fact.value)? == value.object
                    {
                        starts.push(fact.valid_from);
                    }
                }
                let from = starts
                    .into_iter()
                    .max()
                    .ok_or_else(|| invalid("missing ROOT claim"))?;
                (value, from, Some(at.clone()), Op::Retract)
            }
        };
        result.push(Datum {
            entity: crate::rdf::intern_subject(store, &triple.subject)?,
            attribute: store.intern(triple.predicate.as_str())?,
            value: crate::rdf::term_to_value(store, &triple.object)?,
            valid_from: from,
            valid_to: to,
            op,
        });
    }
    Ok(result)
}

/// One savepoint covers prospective validation, receive, decide and pending
/// receipt. A failure at any step rolls all of them back. A no-op writes no tx.
pub(crate) fn execute(
    store: &mut Store,
    ledger: &str,
    prepared: Prepared,
    authored_at: &str,
    td: &str,
    actor: Option<&str>,
) -> Result<Receipt> {
    crate::share_merge::core::atomic(store, |store| {
        if let Some(done) = load(store, ledger, &prepared.id)? {
            return Ok(done);
        }
        // A caller-provided parent id is not resume authority. Reconstruct the
        // exact remainder through the authenticated ledger inside this same
        // savepoint, before any receive, decision or receipt can be written.
        if let Some(parent) = &prepared.request.parent {
            let receipt =
                load(store, ledger, parent)?.ok_or_else(|| invalid("unknown parent receipt"))?;
            if resume_request(store, &receipt)? != prepared.request
                || receipt.authored_at != authored_at
            {
                return Err(invalid(
                    "request or authored time differs from parent remainder",
                ));
            }
        }
        if prepared.authored_at != authored_at || prepared.decision_at != td {
            return Err(invalid(
                "prepared operation clocks differ from persistence clocks",
            ));
        }
        if ledger.trim().is_empty() || store.lookup(ledger)? == Some(crate::schema::ROOT_GRAPH) {
            return Err(invalid("explicit non-ROOT ledger graph required"));
        }
        if prepared.request.path == merge_session::Path::Pendant {
            return Err(invalid("pendant plans are persisted by Git, not the store"));
        }
        if store.store_id()? != prepared.request.store_id
            || store.latest_tx_id()? != prepared.request.root_tx
        {
            return Err(invalid("stale store or transaction head"));
        }
        let (root, _) = crate::share_merge::root_graph(store)?;
        if merge_session::render(&root) != prepared.request.ours {
            return Err(invalid("ROOT snapshot moved"));
        }
        if let Some(parent) = &prepared.request.parent
            && store
                .find_transaction_by_source(&format!("resume_parent={parent};"))?
                .is_some()
        {
            return Err(invalid(
                "parent already has an applied child; resume that child's remainder",
            ));
        }
        #[cfg(feature = "shacl")]
        if !prepared.request.shapes.trim().is_empty() {
            let validation = crate::shacl::Validator::from_turtle(&prepared.request.shapes)?
                .validate(merge_session::render(&prepared.prospective).as_bytes())?;
            if !validation.conforms {
                return Err(invalid("prospective graph does not conform"));
            }
        }
        #[cfg(not(feature = "shacl"))]
        if !prepared.request.shapes.trim().is_empty() {
            return Err(invalid("shape validation feature required"));
        }
        let rows = prepared
            .plan
            .slots
            .iter()
            .map(|(slot, p)| ReportRow {
                slot: slot.clone(),
                held: p.held,
                decision: prepared
                    .choices
                    .iter()
                    .find(|choice| choice.slot == *slot)
                    .map(|choice| choice.decision.clone()),
                kind: report::Kind::from_change(
                    p.change,
                    prepared
                        .request
                        .profile
                        .lists
                        .iter()
                        .any(|b| b.slot == *slot),
                ),
            })
            .collect();
        let mut receipt = Receipt {
            schema: "quipu-partial-merge/v1".into(),
            id: prepared.id.clone(),
            request: prepared.request.clone(),
            authored_at: authored_at.into(),
            receive_tx: None,
            decide_tx: None,
            applied: prepared.plan.applied,
            held: prepared.plan.held,
            rows,
            held_units: prepared.held_units.clone(),
            guards: Vec::new(),
            reviewed_choices: prepared.reviewed_choices,
            store_shapes_hash: crate::share::sha256(
                store.get_combined_shapes()?.unwrap_or_default().as_bytes(),
            ),
        };
        if (prepared.plan.held > 0 && prepared.request.profile.mode == Mode::Atomic)
            || (prepared.receive.is_empty()
                && prepared.decide.is_empty()
                && prepared.plan.held == 0)
        {
            return Ok(receipt);
        }
        // Lower before receive alters any ROOT provenance view.
        let receive = datums(store, &prepared.receive)?;
        let decide = datums(store, &prepared.decide)?;
        let parent = prepared
            .request
            .parent
            .as_ref()
            .map_or(String::new(), |id| format!("resume_parent={id};"));
        let source = format!("merge-session;merge_id={};{parent}", prepared.id);
        if !receive.is_empty() {
            receipt.receive_tx = Some(store.transact(
                &receive,
                td,
                actor,
                Some(&format!("{source}step=receive")),
            )?);
        }
        if !decide.is_empty() {
            receipt.decide_tx =
                Some(store.transact(&decide, td, actor, Some(&format!("{source}step=decide")))?);
        }
        for row in &receipt.rows {
            if row.held {
                receipt.guards.push(Guard {
                    slot: row.slot.clone(),
                    digest: guard(store, &row.slot, &receipt.request)?,
                });
            }
        }
        let payload =
            serde_json::to_string(&receipt).map_err(|e| Error::Serialization(e.to_string()))?;
        if payload.len() > MAX_RECORD {
            return Err(invalid("pending receipt exceeds bound"));
        }
        let datum = Datum {
            entity: store.intern(&entity(&prepared.id))?,
            attribute: store.intern(PAYLOAD)?,
            value: Value::Str(payload),
            valid_from: td.into(),
            valid_to: None,
            op: Op::Assert,
        };
        let graph = store.intern(ledger)?;
        store.transact_to_graph(
            &[datum],
            td,
            actor,
            Some(&format!(
                "merge-session;merge_id={};step=record",
                prepared.id
            )),
            graph,
        )?;
        Ok(receipt)
    })
}

/// Build a fresh child request only after verifying every held slot's actual
/// facts/provenance. Applied slots are excluded, and the child names its parent.
fn resume_request(store: &Store, parent: &Receipt) -> Result<Request> {
    if parent.held == 0 || parent.request.profile.mode == Mode::Atomic {
        return Err(invalid("receipt has no resumable partial remainder"));
    }
    if parent.request.store_id != store.store_id()? {
        return Err(invalid("foreign parent"));
    }
    if parent.store_shapes_hash
        != crate::share::sha256(store.get_combined_shapes()?.unwrap_or_default().as_bytes())
    {
        return Err(invalid("active shapes moved since the partial apply"));
    }
    for proof in &parent.guards {
        if guard(store, &proof.slot, &parent.request)? != proof.digest {
            return Err(invalid("held slot values or provenance moved"));
        }
    }
    let mut request = parent.request.clone();
    request.parent = Some(parent.id.clone());
    request.root_tx = store.latest_tx_id()?;
    request.ours = merge_session::render(&crate::share_merge::root_graph(store)?.0);
    let active: BTreeSet<_> = parent.guards.iter().map(|g| g.slot.clone()).collect();
    request
        .profile
        .units
        .retain(|unit| !unit.is_disjoint(&active));
    if request
        .profile
        .units
        .iter()
        .any(|unit| !unit.is_subset(&active))
    {
        return Err(invalid("partial unit in parent receipt"));
    }
    request.active_slots = Some(active);
    Ok(request)
}

/// Only a receipt read through the admitted ledger may authorize a resume.
pub(crate) fn resume(store: &Store, ledger: &str, id: &str) -> Result<Request> {
    let parent = load(store, ledger, id)?.ok_or_else(|| invalid("unknown parent receipt"))?;
    resume_request(store, &parent)
}

/// Same writer API for both store adapters; request.path keeps identities apart.
pub(crate) fn apply_share(
    store: &mut Store,
    ledger: &str,
    prepared: Prepared,
    authored_at: &str,
    td: &str,
    actor: Option<&str>,
) -> Result<Receipt> {
    if prepared.request.path != merge_session::Path::Share {
        return Err(invalid("not a share plan"));
    }
    execute(store, ledger, prepared, authored_at, td, actor)
}
pub(crate) fn apply_fork(
    store: &mut Store,
    ledger: &str,
    prepared: Prepared,
    authored_at: &str,
    td: &str,
    actor: Option<&str>,
) -> Result<Receipt> {
    if prepared.request.path != merge_session::Path::Fork {
        return Err(invalid("not a fork plan"));
    }
    execute(store, ledger, prepared, authored_at, td, actor)
}
