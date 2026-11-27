//! Slice B, after PREPARE (aegis-n0wc.5; malcolm C1/C2, dearing c69/c71):
//! the prepare budget, the pre-decision shapes re-check, the post-COMMIT
//! replay of judged members and the post-release audit. Split from
//! `participant_derive.rs` for size only.
use super::*;

/// The most time all members' PREPARE dry runs may hold the fences (malcolm
/// C2). Checked at every dry-run boundary: one validator call is not
/// preemptible, so the hold is bounded by this plus one validation.
#[cfg(feature = "shacl")]
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
    static BEFORE_DECISION: std::cell::RefCell<Option<(Option<String>, StoreHook)>> =
        const { std::cell::RefCell::new(None) };
}

/// Test only: every dry run sleeps `delay` (an artificially slow validator).
#[cfg(all(test, feature = "shacl"))]
pub fn with_slow_dry_run<T>(delay: std::time::Duration, work: impl FnOnce() -> T) -> T {
    DRY_RUN_DELAY.with(|slot| slot.set(Some(delay)));
    let result = work();
    DRY_RUN_DELAY.with(|slot| slot.set(None));
    result
}

/// Test only: a smaller PREPARE validation budget, so a slow-validator row
/// need not sleep for the production one.
#[cfg(all(test, feature = "shacl"))]
pub fn with_prepare_budget<T>(budget: std::time::Duration, work: impl FnOnce() -> T) -> T {
    BUDGET.with(|slot| slot.set(Some(budget)));
    let result = work();
    BUDGET.with(|slot| slot.set(None));
    result
}

#[cfg(feature = "shacl")]
pub(super) fn budget() -> std::time::Duration {
    #[cfg(test)]
    if let Some(budget) = BUDGET.with(std::cell::Cell::get) {
        return budget;
    }
    PREPARE_VALIDATION_BUDGET
}

/// Test only: run `change` on the first judged member's writer just before
/// the COMMIT decision (a shapes reload between PREPARE and the decision).
#[cfg(all(test, feature = "shacl"))]
pub fn with_before_decision<T>(
    change: impl FnOnce(&mut Store) + 'static,
    work: impl FnOnce() -> T,
) -> T {
    before_decision(None, Box::new(change), work)
}

/// Test only: [`with_before_decision`] on the member whose file is `file`,
/// judged at PREPARE or not (eph-review-d2b162 #1).
#[cfg(all(test, feature = "shacl"))]
pub fn with_before_decision_on<T>(
    file: &str,
    change: impl FnOnce(&mut Store) + 'static,
    work: impl FnOnce() -> T,
) -> T {
    before_decision(Some(file.to_owned()), Box::new(change), work)
}

#[cfg(all(test, feature = "shacl"))]
fn before_decision<T>(file: Option<String>, hook: StoreHook, work: impl FnOnce() -> T) -> T {
    BEFORE_DECISION.with(|slot| *slot.borrow_mut() = Some((file, hook)));
    let result = work();
    BEFORE_DECISION.with(|slot| *slot.borrow_mut() = None);
    result
}

/// Immediately before the COMMIT decision (intents prepared, nothing
/// decided): every member must still carry exactly the write shapes its
/// PREPARE saw: the same bytes for a judged member, and still none for a
/// member that had none (eph-review-d2b162 #1: shapes loaded into an unjudged
/// member would otherwise meet the full gate after COMMIT, the B0 strand). A
/// change refuses here, so the caller aborts every intent instead of meeting
/// the change after COMMIT (malcolm C1). Validation-only participants and
/// type sources write no facts, so their shapes do not judge anything.
pub(in super::super) fn recheck_shapes(
    order: &[&GraphSetOwner],
    guards: &mut [Guard<'_>],
    plan: &BTreeMap<String, PinChanges>,
    members: &[Member<'_>],
) -> Result<()> {
    #[cfg(test)]
    let mut hook = BEFORE_DECISION.with(|slot| slot.borrow_mut().take());
    for (owner, store) in order.iter().zip(guards.iter_mut()) {
        if !members.iter().any(|m| std::ptr::eq(m.owner, *owner)) {
            continue;
        }
        let file = &owner.owner.catalog_route().file_id;
        let expected = plan
            .get(file)
            .and_then(|p| p.shapes.as_ref())
            .map(|(sha, _)| sha);
        #[cfg(test)]
        if hook.as_ref().is_some_and(|(target, _)| match target {
            Some(target) => target == file,
            None => expected.is_some(),
        }) {
            let (_, change) = hook.take().expect("checked");
            change(store);
        }
        if shapes_of(store)?.map(|s| sha256(s.as_bytes())).as_ref() != expected {
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
#[cfg(all(test, feature = "shacl"))]
pub(super) fn test_delay() {
    if let Some(delay) = DRY_RUN_DELAY.with(std::cell::Cell::get) {
        std::thread::sleep(delay);
    }
}

/// APPLY of a committed intent (dearing c69/c71): a member judged at PREPARE
/// is replayed WITHOUT re-running the write SHACL gate. The decision is final;
/// a post-commit refusal would strand every participant (B0). Verification is
/// the post-release [`audit`]. A member never judged applies unchanged.
///
/// Turning `validate_on_write` off for the apply also silences warn-policy
/// shape REPORTING for that member's write (eph-review-873d46 #3): the
/// PREPARE dry run already ran both reject and warn policies on the exact
/// batch, and the audit re-verifies under the carried bytes, so a judged
/// member's warn results are those of its PREPARE judgement, not re-emitted.
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

/// After PUBLISHED (dearing c71): verify each judged member against the shape
/// BYTES its intent carried, and record when the current shapes differ. Never
/// refuses and never re-fences: a failure can only mean a corrupt intent or a
/// replay bug, so it is a loud, durable 'verify-failed' record (and a
/// server-log line) while the member stays applied. A pass is a durable
/// 'verified' record, so every judged member ends with exactly one outcome.
///
/// Runs BEFORE the fence release (eph-review-0dd73c #1): the audit is the only
/// check a judged member gets after COMMIT, and a crash before its outcome row
/// must leave an applied intent behind so recovery is forced through the
/// published arm, which calls [`audit_pending`]. Reads already see the new cut
/// once the home journal is published, so holding the fences here costs no
/// read visibility, and writes are excluded by the held writers anyway.
pub(in super::super) fn audit(
    guards: &mut [Guard<'_>],
    batch_id: &str,
    members: &[Recorded],
    intents: &[(IntentRow, Decoded)],
) {
    for ((store, (_, payload)), member) in guards.iter_mut().zip(intents).zip(members) {
        audit_member(store, batch_id, member, payload);
    }
}

/// Recovery of a PUBLISHED batch: audit every judged member that has no
/// outcome row yet, from its durable intent alone. Idempotent: a member that
/// already carries 'verified' or 'verify-failed' is not re-verified. An intent
/// that cannot be read or no longer matches the home journal is itself a loud
/// 'verify-failed' record; it never refuses the release.
pub(in super::super) fn audit_pending(
    guards: &mut [Guard<'_>],
    batch_id: &str,
    members: &[Recorded],
    digest: &str,
) {
    for (index, (store, member)) in guards.iter_mut().zip(members).enumerate() {
        if audited(store, batch_id) {
            continue;
        }
        let decoded = match intent(store, batch_id) {
            Ok(Some(row))
                if row.batch_digest == digest
                    && row.member == index as i64
                    && row.digest
                        == intent_digest(digest, index, row.expected_head, &row.payload) =>
            {
                decode_payload(&row.payload).map_err(|error| error.to_string())
            }
            Ok(Some(_)) => Err("intent no longer matches the home journal".into()),
            Ok(None) => Err("published batch lacks this member's intent".into()),
            Err(error) => Err(error.to_string()),
        };
        match decoded {
            Ok(payload) => audit_member(store, batch_id, member, &payload),
            Err(problem) => {
                let detail = format!("member {}: {problem}; cannot audit", member.file_id);
                eprintln!("quipu participant audit FAILED batch={batch_id} {detail}");
                note(store, batch_id, "verify-failed", "unknown", None, &detail);
            }
        }
    }
}

/// Whether this member already has an audit outcome for the batch. An
/// unreadable answer counts as not audited: a repeat audit only rewrites rows.
fn audited(store: &Store, batch_id: &str) -> bool {
    store
        .conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM participant_audit WHERE batch_id=?1 \
             AND kind IN ('verified','verify-failed'))",
            [batch_id],
            |row| row.get(0),
        )
        .unwrap_or(false)
}

fn audit_member(store: &mut Store, batch_id: &str, member: &Recorded, payload: &Decoded) {
    let Some((judged, bytes)) = &payload.pins.shapes else {
        return;
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
    match verify(store, payload, judged, bytes) {
        Ok(()) => note(
            store,
            batch_id,
            "verified",
            judged,
            current.as_deref(),
            &format!(
                "member {}: verifies under the carried shapes",
                member.file_id
            ),
        ),
        Err(problem) => {
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
