//! Explicit profiled adapters. No CLI/MCP default or installed profile changes.
use crate::error::{Error, Result};
use crate::merge_lists::Slot;
use crate::merge_pending::{self, Receipt};
use crate::merge_session::{self, Choice, Path, Prepared, Profile, Request};
use crate::store::Store;

fn invalid(message: &str) -> Error {
    Error::InvalidValue(format!("merge adapter: {message}"))
}

pub(crate) fn share_request(
    store: &Store,
    incoming: &std::path::Path,
    profile: Profile,
) -> Result<(Request, String)> {
    let incoming = crate::share_merge::read_share(incoming)?;
    let base = crate::share_merge::locate_base(&incoming)?;
    Ok((
        Request {
            store_id: store.store_id()?,
            root_tx: store.latest_tx_id()?,
            path: Path::Share,
            input_id: incoming.manifest.share_id,
            base_id: base.manifest.share_id,
            base: merge_session::render(&base.graph),
            ours: merge_session::render(&crate::share_merge::root_graph(store)?.0),
            theirs: merge_session::render(&incoming.graph),
            shapes: incoming.shapes,
            profile,
            parent: None,
            active_slots: None,
            scope: None,
        },
        incoming.manifest.created_at,
    ))
}

pub(crate) fn fork_request(
    store: &Store,
    name: &str,
    profile: Profile,
) -> Result<(Request, String)> {
    let (fork, base, ours, theirs) = store.fork_session_snapshot(name)?;
    // Per-fact fork clocks are read by prepare_store, not guessed from wall time.
    Ok((
        Request {
            store_id: store.store_id()?,
            root_tx: store.latest_tx_id()?,
            path: Path::Fork,
            input_id: fork.name,
            base_id: fork.fork_tx.to_string(),
            base,
            ours,
            theirs,
            shapes: store.get_combined_shapes()?.unwrap_or_default(),
            profile,
            parent: None,
            active_slots: None,
            scope: None,
        },
        fork.created_at,
    ))
}

pub(crate) fn prepare_store(
    store: &Store,
    request: Request,
    choices: &[Choice],
    authored_at: &str,
    td: &str,
) -> Result<Prepared> {
    if request.path == Path::Pendant {
        return Err(invalid("Git adapter required"));
    }
    let fork = if request.path == Path::Fork {
        Some(request.input_id.clone())
    } else {
        None
    };
    merge_session::prepare(
        request,
        choices,
        authored_at,
        td,
        |incoming, slot, value| {
            if incoming {
                match &fork {
                    Some(name) => fact_start(store, store.fork_graph_for_read(name)?, slot, value),
                    None => Ok(authored_at.to_owned()),
                }
            } else {
                fact_start(store, crate::schema::ROOT_GRAPH, slot, value)
            }
        },
        |slot| match &fork {
            Some(name) => store.fork_session_closes(name, slot),
            None => Ok(Default::default()),
        },
    )
}

fn fact_start(store: &Store, graph: i64, slot: &Slot, value: &str) -> Result<String> {
    let subject = slot
        .0
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .ok_or_else(|| invalid("stable subject required"))?;
    let (Some(e), Some(a)) = (store.lookup(subject)?, store.lookup(&slot.1)?) else {
        return Err(invalid("missing ROOT slot"));
    };
    let mut times = Vec::new();
    for f in store.entity_facts_in_graph(e, graph)? {
        if f.attribute == a && crate::rdf::value_to_term(store, &f.value)?.to_string() == value {
            times.push(f.valid_from);
        }
    }
    times
        .into_iter()
        .max()
        .ok_or_else(|| invalid("missing ROOT provenance"))
}

pub(crate) fn apply_store(
    store: &mut Store,
    ledger: &str,
    prepared: Prepared,
    authored_at: &str,
    td: &str,
    actor: Option<&str>,
) -> Result<Receipt> {
    match prepared.request.path {
        Path::Share => merge_pending::apply_share(store, ledger, prepared, authored_at, td, actor),
        Path::Fork => crate::share_merge::core::atomic(store, |store| {
            if let Some(done) = merge_pending::load(store, ledger, &prepared.id)? {
                return Ok(done);
            }
            let name = prepared.request.input_id.clone();
            // A fresh retained file is a proposal, not authority over the fork
            // snapshot: a forged base turns a ROOT-only fact into an incoming
            // deletion. Rebuild the request and clock here, inside the writer's
            // savepoint, before any receive, decision or receipt. Resumed files
            // are bound to their authenticated parent receipt in execute.
            if prepared.request.parent.is_none() {
                let (actual, authored) =
                    fork_request(store, &name, prepared.request.profile.clone())?;
                if actual != prepared.request || authored != authored_at {
                    return Err(invalid("session differs from the current fork snapshot"));
                }
            }
            let (fork, _, _, current) = store.fork_session_snapshot(&name)?;
            if fork.status != "open"
                || current != prepared.request.theirs
                || fork.fork_tx.to_string() != prepared.request.base_id
            {
                return Err(invalid("fork changed or closed"));
            }
            let receipt =
                merge_pending::apply_fork(store, ledger, prepared, authored_at, td, actor)?;
            if receipt.held == 0 {
                store.finish_fork_session(&name, &receipt.id, td)?;
            }
            Ok(receipt)
        }),
        Path::Pendant => Err(invalid("Git owns pendant persistence")),
    }
}

/// Resume from the durable ledger, re-reading ROOT and incoming fork state.
pub(crate) fn resume(
    store: &Store,
    ledger: &str,
    parent: &str,
    choices: &[Choice],
    td: &str,
) -> Result<Prepared> {
    let receipt =
        merge_pending::load(store, ledger, parent)?.ok_or_else(|| invalid("missing parent"))?;
    let request = merge_pending::resume(store, ledger, parent)?;
    if request.path == Path::Fork
        && store.fork_session_snapshot(&request.input_id)?.3 != request.theirs
    {
        return Err(invalid("incoming fork moved"));
    }
    prepare_store(store, request, choices, &receipt.authored_at, td)
}

/// Git uses the same planner, with no valid-time/transaction data in the pack.
pub(crate) fn pendant(request: Request, choices: &[Choice]) -> Result<Prepared> {
    if request.path != Path::Pendant {
        return Err(invalid("not a pendant request"));
    }
    let mut prepared = merge_session::prepare(
        request,
        choices,
        "",
        "",
        |_, _, _| Ok(String::new()),
        |_| Ok(Default::default()),
    )?;
    let base = crate::share_merge::parse_graph(&prepared.request.base, "pendant base")?;
    // A held pendant unit is inspectable at base, with no received incoming cell.
    for (slot, plan) in &prepared.plan.slots {
        if !plan.held {
            continue;
        }
        let declaration = prepared
            .request
            .profile
            .lists
            .iter()
            .find(|b| b.slot == *slot);
        if declaration.is_some() {
            let all = prepared
                .request
                .profile
                .lists
                .iter()
                .map(|b| (b.slot.clone(), b.mode))
                .collect();
            let old = crate::merge_lists::read(&prepared.prospective, &all)?;
            let cells = old
                .get(slot)
                .and_then(Option::as_ref)
                .map(|v| v.cells.clone())
                .unwrap_or_default();
            prepared.prospective.retain(|t| {
                !(t.subject.to_string() == slot.0 && t.predicate.as_str() == slot.1)
                    && !cells.contains(&t.subject.to_string())
            });
            let originals = crate::merge_lists::read(&base, &all)?;
            let sequence = originals[slot].as_ref().map(|v| v.elements.clone());
            prepared
                .prospective
                .extend(crate::merge_lists::lower(slot, &sequence)?);
        } else {
            prepared
                .prospective
                .retain(|t| !(t.subject.to_string() == slot.0 && t.predicate.as_str() == slot.1));
            prepared.prospective.extend(
                base.iter()
                    .filter(|t| t.subject.to_string() == slot.0 && t.predicate.as_str() == slot.1)
                    .cloned(),
            );
        }
    }
    Ok(prepared)
}
