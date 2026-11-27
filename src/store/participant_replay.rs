//! Slice B, after PREPARE (aegis-n0wc.5; malcolm C1/C2, dearing c69/c71):
//! the prepare budget, the pre-decision shapes re-check, the post-COMMIT
//! replay of judged members and the post-release audit. Split from
//! `participant_derive.rs` for size only.
use super::*;

/// The most time all members' PREPARE dry runs may hold the fences (malcolm
/// C2). Checked at every dry-run boundary: one validator call is not
/// preemptible, so the hold is bounded by this plus one validation.
pub const PREPARE_VALIDATION_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);

/// Test only: a change applied to one member's writer at a chosen point.
#[cfg(test)]
type StoreHook = Box<dyn FnOnce(&mut Store)>;

#[cfg(test)]
thread_local! {
    static DRY_RUN_DELAY: std::cell::Cell<Option<std::time::Duration>> =
        const { std::cell::Cell::new(None) };
    static BUDGET: std::cell::Cell<Option<std::time::Duration>> =
        const { std::cell::Cell::new(None) };
    static BEFORE_DECISION: std::cell::RefCell<Option<StoreHook>> =
        const { std::cell::RefCell::new(None) };
}

/// Test only: every dry run sleeps `delay` (an artificially slow validator).
#[cfg(test)]
pub fn with_slow_dry_run<T>(delay: std::time::Duration, work: impl FnOnce() -> T) -> T {
    DRY_RUN_DELAY.with(|slot| slot.set(Some(delay)));
    let result = work();
    DRY_RUN_DELAY.with(|slot| slot.set(None));
    result
}

/// Test only: a smaller PREPARE validation budget, so a slow-validator row
/// need not sleep for the production one.
#[cfg(test)]
pub fn with_prepare_budget<T>(budget: std::time::Duration, work: impl FnOnce() -> T) -> T {
    BUDGET.with(|slot| slot.set(Some(budget)));
    let result = work();
    BUDGET.with(|slot| slot.set(None));
    result
}

pub(super) fn budget() -> std::time::Duration {
    #[cfg(test)]
    if let Some(budget) = BUDGET.with(std::cell::Cell::get) {
        return budget;
    }
    PREPARE_VALIDATION_BUDGET
}

/// Test only: run `change` on the first judged member's writer just before
/// the COMMIT decision (a shapes reload between PREPARE and the decision).
#[cfg(test)]
pub fn with_before_decision<T>(
    change: impl FnOnce(&mut Store) + 'static,
    work: impl FnOnce() -> T,
) -> T {
    BEFORE_DECISION.with(|slot| *slot.borrow_mut() = Some(Box::new(change)));
    let result = work();
    BEFORE_DECISION.with(|slot| *slot.borrow_mut() = None);
    result
}

/// Immediately before the COMMIT decision (intents prepared, nothing
/// decided): every judged member must still be judged by exactly the shapes
/// its PREPARE dry run used. A change refuses here, so the caller aborts every
/// intent instead of meeting the change after COMMIT (malcolm C1).
pub(in super::super) fn recheck_shapes(
    order: &[&GraphSetOwner],
    guards: &mut [Guard<'_>],
    plan: &BTreeMap<String, PinChanges>,
) -> Result<()> {
    #[cfg(test)]
    let mut hook = BEFORE_DECISION.with(|slot| slot.borrow_mut().take());
    for (owner, store) in order.iter().zip(guards.iter_mut()) {
        let file = &owner.owner.catalog_route().file_id;
        let Some((expected, _)) = plan.get(file).and_then(|p| p.shapes.as_ref()) else {
            continue;
        };
        #[cfg(test)]
        if let Some(change) = hook.take() {
            change(store);
        }
        if shapes_of(store)?.map(|s| sha256(s.as_bytes())).as_ref() != Some(expected) {
            return Err(refuse(format!(
                "member {file}: write shapes changed between PREPARE and the decision; \
                 batch aborted before commit"
            )));
        }
    }
    Ok(())
}

pub(super) fn shapes_of(store: &Store) -> Result<Option<String>> {
    #[cfg(feature = "shacl")]
    return store.write_reject_shapes();
    #[cfg(not(feature = "shacl"))]
    {
        let _ = store;
        Ok(None)
    }
}

/// Test only: the injected dry-run delay, if armed.
#[cfg(test)]
pub(super) fn test_delay() {
    if let Some(delay) = DRY_RUN_DELAY.with(std::cell::Cell::get) {
        std::thread::sleep(delay);
    }
}

/// APPLY of a committed intent (dearing c69/c71): a member judged at PREPARE
/// is replayed WITHOUT re-running the write SHACL gate. The decision is final;
/// a post-commit refusal would strand every participant (B0). Verification is
/// the post-release [`audit`]. A member never judged applies unchanged.
pub(in super::super) fn replay<T>(
    store: &mut Store,
    pins: &PinChanges,
    operation: impl FnOnce(&mut Store) -> Result<T>,
) -> Result<T> {
    if pins.shapes.is_none() {
        return store.with_derived_type_context(&pins.context, operation);
    }
    let validate = std::mem::replace(&mut store.shacl_config.validate_on_write, false);
    let explicit = store.write_shapes.take();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(store)));
    store.shacl_config.validate_on_write = validate;
    store.write_shapes = explicit;
    match result {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// After PUBLISHED and the fence release (dearing c71): verify each judged
/// member against the shape BYTES its intent carried, and record when the
/// current shapes differ. Never refuses and never re-fences: a failure can
/// only mean a corrupt intent or a replay bug, so it is a loud, durable
/// 'verify-failed' record (and a server-log line) while the member stays applied.
pub(in super::super) fn audit(
    guards: &mut [Guard<'_>],
    batch_id: &str,
    members: &[Recorded],
    intents: &[(IntentRow, Decoded)],
) {
    for ((store, (_, payload)), member) in guards.iter_mut().zip(intents).zip(members) {
        let Some((judged, bytes)) = &payload.pins.shapes else {
            continue;
        };
        let current = shapes_of(store)
            .ok()
            .flatten()
            .map(|s| sha256(s.as_bytes()));
        if current.as_ref() != Some(judged) {
            let detail = format!(
                "judged under {judged} (bytes carried), current {}",
                current.as_deref().unwrap_or("none")
            );
            note(
                store,
                batch_id,
                "shapes-delta",
                judged,
                current.as_deref(),
                &detail,
            );
        }
        if let Err(problem) = verify(store, payload, judged, bytes) {
            let detail = format!("member {}: {problem}; stays applied", member.file_id);
            eprintln!("quipu participant audit FAILED batch={batch_id} {detail}");
            note(
                store,
                batch_id,
                "verify-failed",
                judged,
                current.as_deref(),
                &detail,
            );
        }
    }
}

fn verify(
    store: &mut Store,
    payload: &Decoded,
    judged: &str,
    bytes: &str,
) -> std::result::Result<(), String> {
    if sha256(bytes.as_bytes()) != judged {
        return Err("carried shape bytes are missing or do not match their digest".into());
    }
    #[cfg(feature = "shacl")]
    {
        let validate = std::mem::replace(&mut store.shacl_config.validate_on_write, false);
        let explicit = store.write_shapes.take();
        let delta = Delta {
            graphs: &payload.graphs,
            timestamp: &payload.timestamp,
            actor: payload.actor.as_deref(),
            source: payload.source.as_deref(),
        };
        let outcome = store.with_write_shapes(Some(bytes), |store| {
            dry_run(store, &delta, &payload.pins.context)
        });
        store.shacl_config.validate_on_write = validate;
        store.write_shapes = explicit;
        outcome.map_err(|error| format!("does not verify under the carried shapes: {error}"))
    }
    #[cfg(not(feature = "shacl"))]
    {
        let _ = (store, payload);
        Err("cannot verify carried shapes without the shacl feature".into())
    }
}

fn note(
    store: &Store,
    batch_id: &str,
    kind: &str,
    judged: &str,
    current: Option<&str>,
    detail: &str,
) {
    let written = store.conn.execute(
        "INSERT OR REPLACE INTO participant_audit(batch_id, kind, judged_sha256, \
         current_sha256, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![batch_id, kind, judged, current, detail],
    );
    if let Err(error) = written {
        eprintln!(
            "quipu participant audit record NOT written batch={batch_id} kind={kind}: {error}"
        );
    }
}
