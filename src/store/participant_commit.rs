//! Participant commit across graph-set files (aegis-n0wc.5 Phase 3, S1).
//!
//! Native, source-only and default-off: no route, configuration or migration
//! enables it. It implements the design's "Write atomicity and crash protocol"
//! (docs/design/per-graph-writable-stores.md) for a batch whose members are
//! distinct [`GraphSetOwner`]s of ONE pinned catalog:
//!
//! 1. Bounded try-all admission of every participant writer in canonical
//!    (`file_id`) order, then one immutable prepared intent per participant
//!    (exact payload, expected local head, home identity, digest).
//! 2. The home participant (first in canonical order) records COMMIT in its
//!    journal. Before it, recovery aborts; after it, recovery must finish the
//!    same batch with the same payloads and never generates new identities.
//! 3. Each intent is applied once, facts and the `applied` receipt in ONE local
//!    SQLite transaction through the ordinary guarded batch pipeline.
//! 4. The home journal's PUBLISHED record (complete receipt vector) is the
//!    external commit point. Retries return that same vector.
//!
//! Visibility is conservative: an applied but unpublished intent makes every
//! owner read of that file refuse (durably, across restart) until recovery
//! reconciles the home decision; a prepared or applied intent refuses ordinary
//! writes. Unrelated files are unaffected. Not provided here: cross-graph
//! constraint dependencies, signed or scoped requests, embedding/reactive
//! completion, composed-reader publication cuts, any HTTP surface.
use super::*;
use serde::{Deserialize, Serialize};

#[path = "participant_records.rs"]
mod records;
use records::*;
pub(crate) use records::{check_read_fence, check_write_fence};

/// native delta (term IDs local to that owner's file).
pub struct Member<'a> {
    pub owner: &'a GraphSetOwner,
    pub requests: Vec<WriteRequest<'a>>,
    pub batch: GraphSetBatch,
}

/// A caller-chosen durable batch identity and its members (any order).
pub struct ParticipantBatch<'a> {
    pub batch_id: String,
    pub members: Vec<Member<'a>>,
}

/// One participant's local receipt: its identity and graph-scoped local heads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberReceipt {
    pub file_id: String,
    pub store_id: String,
    pub log_epoch: String,
    pub heads: Vec<(String, i64)>,
}

/// The complete published receipt vector, in canonical participant order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptVector {
    pub batch_id: String,
    pub digest: String,
    pub members: Vec<MemberReceipt>,
}

/// The authoritative outcome recovery reconciled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// No COMMIT decision was durable: every participant keeps its old cut.
    Aborted,
    /// The complete new cut is published.
    Published(ReceiptVector),
}

/// Durable edges where a test injects a crash (an early return that skips all
/// cleanup). Indexes are canonical participant positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashPoint {
    AfterJournal,
    AfterPrepare(usize),
    AfterDecision,
    AfterApply(usize),
    AfterPublish,
}

#[cfg(test)]
thread_local! {
    static CRASH: std::cell::Cell<Option<CrashPoint>> = const { std::cell::Cell::new(None) };
    static CRASHED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run `work` with one injected crash point armed (test only).
#[cfg(test)]
pub fn with_crash<T>(point: CrashPoint, work: impl FnOnce() -> T) -> T {
    CRASH.with(|slot| slot.set(Some(point)));
    CRASHED.with(|slot| slot.set(false));
    let result = work();
    CRASH.with(|slot| slot.set(None));
    result
}

fn crash(point: CrashPoint) -> Result<()> {
    // A child test process armed by the parent reports here, then blocks
    // holding every writer and lease until it is SIGKILLed (test only).
    #[cfg(test)]
    if std::env::var(kill_tests::KILL_ENV).ok().as_deref() == Some(&format!("{point:?}")) {
        use std::io::Write;
        println!("{}", kill_tests::MARKER);
        let _ = std::io::stdout().flush();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    #[cfg(test)]
    if CRASH.with(std::cell::Cell::get) == Some(point) {
        CRASHED.with(|slot| slot.set(true));
        return Err(Error::PolicyDenied(format!(
            "injected participant crash at {point:?}"
        )));
    }
    let _ = point;
    Ok(())
}

/// True only after an injected crash: the simulated process skips cleanup.
fn crashed() -> bool {
    #[cfg(test)]
    return CRASHED.with(std::cell::Cell::get);
    #[cfg(not(test))]
    false
}

fn refuse(message: impl std::fmt::Display) -> Error {
    refused(format!("participant commit: {message}"))
}

type Guard<'a> = parking_lot::FairMutexGuard<'a, Store>;

/// Bounded try-all admission in canonical order: never wait in one
/// participant's queue while holding another's writer. Every early return
/// drops everything acquired so far; nothing is prepared before all succeed.
fn admit_all<'a>(owners: &[&'a GraphSetOwner]) -> Result<(Vec<Admission<'a>>, Vec<Guard<'a>>)> {
    let mut admissions = Vec::with_capacity(owners.len());
    let mut guards = Vec::with_capacity(owners.len());
    for owner in owners {
        admissions.push(owner.owner.admit()?);
        let guard = owner
            .owner
            .writer
            .try_owned()?
            .ok_or_else(|| refuse("participant writer busy; nothing prepared"))?;
        owner.owner.placement.verify()?;
        Owner::check_identity(owner.owner.catalog_route(), &guard)?;
        GraphSetOwner::check_hooks(&guard)?;
        let synchronous: i64 = guard
            .conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))?;
        if synchronous < 2 {
            return Err(refuse("participant durability requires synchronous=FULL"));
        }
        guards.push(guard);
    }
    Ok((admissions, guards))
}

fn canonical<'a>(owners: &[&'a GraphSetOwner]) -> Result<Vec<&'a GraphSetOwner>> {
    let first = owners
        .first()
        .ok_or_else(|| refuse("a participant batch spans at least two files"))?;
    if owners
        .iter()
        .any(|owner| !Arc::ptr_eq(&owner.catalog, &first.catalog))
    {
        return Err(refuse("participants cross pinned catalog generations"));
    }
    let mut sorted = owners.to_vec();
    sorted.sort_by(|a, b| {
        a.owner
            .catalog_route()
            .file_id
            .cmp(&b.owner.catalog_route().file_id)
    });
    if sorted
        .windows(2)
        .any(|pair| pair[0].owner.catalog_route().file_id == pair[1].owner.catalog_route().file_id)
    {
        return Err(refuse("duplicate participant file"));
    }
    Ok(sorted)
}

fn principal() -> Result<crate::transaction_auth::Identity> {
    let identity = crate::transaction_auth::current_identity()
        .ok_or_else(|| refuse("a verified principal is required"))?;
    if crate::transaction_auth::current_attestation().is_some() {
        return Err(refuse("signed participant batches are not qualified"));
    }
    if crate::writer_scope::is_scoped_request() {
        return Err(refuse("scoped participant batches are not qualified"));
    }
    Ok(identity)
}

/// Execute one batch, or return its original receipt vector on retry.
///
/// The caller's verified principal (an owned identity scope) is bound into the
/// batch digest. The same ID with any different byte, member or principal
/// refuses. An aborted ID stays consumed. A refused admission consumes nothing.
pub fn commit(batch: &ParticipantBatch<'_>) -> Result<ReceiptVector> {
    let identity = principal()?;
    if batch.batch_id.is_empty() {
        return Err(refuse("empty batch id"));
    }
    if batch.members.len() < 2 {
        return Err(refuse("a participant batch spans at least two files"));
    }
    let owners: Vec<_> = batch.members.iter().map(|m| m.owner).collect();
    let order = canonical(&owners)?;
    // Validate every member against its own owner before any fence.
    let mut planned = Vec::with_capacity(order.len());
    for owner in &order {
        let member = batch
            .members
            .iter()
            .find(|m| std::ptr::eq(m.owner, *owner))
            .ok_or_else(|| refuse("participant lookup"))?;
        owner.check_batch(&member.requests, &member.batch)?;
        planned.push((recorded(owner), encode_payload(&member.batch)));
    }
    let digest = sha256(
        serde_json::json!({
            "contract": CONTRACT,
            "batch_id": batch.batch_id,
            "principal": identity.principal,
            "credential_id": identity.credential_id,
            "auth_class": identity.auth_class,
            "catalog_revision": order[0].catalog.physical().revision(),
            "members": planned.iter().map(|(r, payload)| serde_json::json!({
                "file_id": r.file_id,
                "store_id": r.store_id,
                "log_epoch": r.log_epoch,
                "payload": sha256(payload),
            })).collect::<Vec<_>>(),
        })
        .to_string()
        .as_bytes(),
    );
    let (_admissions, mut guards) = admit_all(&order)?;
    if let Some(existing) = journal(&guards[0], &batch.batch_id)? {
        if existing.digest != digest {
            return Err(refuse("batch id names a different batch; nothing written"));
        }
        return match reconcile(&order, &mut guards, &batch.batch_id, &existing)? {
            Recovery::Published(vector) => Ok(vector),
            Recovery::Aborted => Err(refuse("batch was aborted; its id stays consumed")),
        };
    }
    for (store, owner) in guards.iter().zip(&order) {
        check_write_fence(store)?;
        if intent(store, &batch.batch_id)?.is_some() {
            return Err(refuse(format!(
                "{} holds an intent for this id without a home journal",
                owner.owner.catalog_route().file_id
            )));
        }
    }
    let members: Vec<_> = planned.iter().map(|(r, _)| r.clone()).collect();
    let home = members[0].clone();
    let encoded = serde_json::to_string(&members).map_err(refuse)?;
    durable(&mut guards[0], |store| {
        store.conn.execute(
            "INSERT INTO participant_journal(batch_id, batch_digest, members, state) \
             VALUES (?1, ?2, ?3, 'preparing')",
            rusqlite::params![batch.batch_id, digest, encoded],
        )?;
        Ok(())
    })?;
    let prepared = (|| {
        crash(CrashPoint::AfterJournal)?;
        for (index, (store, (_, payload))) in guards.iter_mut().zip(&planned).enumerate() {
            let head = store.latest_tx_id()?;
            let intent = intent_digest(&digest, index, head, payload);
            durable(store, |store| {
                store.conn.execute(
                    "INSERT INTO participant_intents(batch_id, batch_digest, home_file_id, \
                     home_store_id, home_log_epoch, member, expected_head, payload, \
                     intent_digest, state) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'prepared')",
                    rusqlite::params![
                        batch.batch_id,
                        digest,
                        home.file_id,
                        home.store_id,
                        home.log_epoch,
                        index as i64,
                        head,
                        payload,
                        intent
                    ],
                )?;
                Ok(())
            })?;
            crash(CrashPoint::AfterPrepare(index))?;
        }
        Ok(())
    })();
    if let Err(error) = prepared {
        if !crashed() {
            // No decision exists yet: abort every preparation now.
            abort(&mut guards, &batch.batch_id)?;
        }
        return Err(error);
    }
    durable(&mut guards[0], |store| {
        let changed = store.conn.execute(
            "UPDATE participant_journal SET state='committed' \
             WHERE batch_id=?1 AND state='preparing'",
            [&batch.batch_id],
        )?;
        if changed != 1 {
            return Err(refuse("home journal changed during preparation"));
        }
        Ok(())
    })?;
    crash(CrashPoint::AfterDecision)?;
    let existing = journal(&guards[0], &batch.batch_id)?
        .ok_or_else(|| refuse("home journal missing after decision"))?;
    match reconcile(&order, &mut guards, &batch.batch_id, &existing)? {
        Recovery::Published(vector) => Ok(vector),
        Recovery::Aborted => Err(refuse("committed batch reconciled as aborted")),
    }
}

/// Reconcile one batch after restart from durable state only (no caller
/// payload). The home must be among `owners`; every recorded participant
/// must be present with its recorded identity. Otherwise participants stay
/// fenced and this refuses.
pub fn recover(owners: &[&GraphSetOwner], batch_id: &str) -> Result<Recovery> {
    let order = canonical(owners)?;
    let (_admissions, mut guards) = admit_all(&order)?;
    let mut home = None;
    for store in &guards {
        if let Some(row) = intent(store, batch_id)? {
            match &home {
                None => home = Some(row.home),
                Some(seen) if *seen != row.home => {
                    return Err(refuse("intents disagree on their home participant"));
                }
                Some(_) => {}
            }
        }
    }
    // A crash between the home journal row and the first intent leaves only
    // the journal: its file is the home.
    if home.is_none() {
        for (owner, store) in order.iter().zip(&guards) {
            if journal(store, batch_id)?.is_some() {
                home = Some(recorded(owner));
            }
        }
    }
    let home = home.ok_or_else(|| refuse("no participant intent or journal for this batch"))?;
    let at = order
        .iter()
        .position(|owner| recorded(owner) == home)
        .ok_or_else(|| refuse("home participant unavailable; participants stay fenced"))?;
    if at != 0 {
        return Err(refuse("home participant is not first in canonical order"));
    }
    let existing = journal(&guards[0], batch_id)?
        .ok_or_else(|| refuse("home participant has no journal for this batch"))?;
    reconcile(&order, &mut guards, batch_id, &existing)
}

fn abort(guards: &mut [Guard<'_>], batch_id: &str) -> Result<()> {
    for store in guards.iter_mut() {
        durable(store, |store| {
            store.conn.execute(
                "UPDATE participant_intents SET state='aborted' \
                 WHERE batch_id=?1 AND state='prepared'",
                [batch_id],
            )?;
            Ok(())
        })?;
    }
    durable(&mut guards[0], |store| {
        store.conn.execute(
            "UPDATE participant_journal SET state='aborted' \
             WHERE batch_id=?1 AND state IN ('preparing','aborted')",
            [batch_id],
        )?;
        Ok(())
    })
}

/// Finish whatever the home journal decided, under every participant writer.
fn reconcile(
    order: &[&GraphSetOwner],
    guards: &mut [Guard<'_>],
    batch_id: &str,
    existing: &JournalRow,
) -> Result<Recovery> {
    let members: Vec<_> = order.iter().map(|owner| recorded(owner)).collect();
    if existing.members != members {
        return Err(refuse(
            "participant set or identity differs from the home journal; participants stay fenced",
        ));
    }
    match existing.state.as_str() {
        "preparing" | "aborted" => {
            abort(guards, batch_id)?;
            Ok(Recovery::Aborted)
        }
        "committed" => {
            // Verify EVERY intent before applying any.
            let mut intents = Vec::with_capacity(guards.len());
            for (index, store) in guards.iter().enumerate() {
                let row = intent(store, batch_id)?
                    .ok_or_else(|| refuse("committed batch lacks a participant intent"))?;
                if row.batch_digest != existing.digest
                    || row.home != members[0]
                    || row.member != index as i64
                    || row.digest
                        != intent_digest(&existing.digest, index, row.expected_head, &row.payload)
                {
                    return Err(refuse(
                        "intent digest, member or home identity changed; participants stay fenced",
                    ));
                }
                let payload = decode_payload(&row.payload)?;
                intents.push((row, payload));
            }
            let mut receipts = Vec::with_capacity(guards.len());
            for (index, (store, (row, payload))) in guards.iter_mut().zip(intents).enumerate() {
                let heads = match row.state.as_str() {
                    "applied" | "published" => serde_json::from_str(
                        row.heads
                            .as_deref()
                            .ok_or_else(|| refuse("applied intent lacks heads"))?,
                    )
                    .map_err(|_| refuse("damaged intent heads"))?,
                    "prepared" => apply(store, batch_id, &row, &payload)?,
                    _ => return Err(refuse("committed batch has an aborted intent")),
                };
                if row.state == "prepared" {
                    crash(CrashPoint::AfterApply(index))?;
                }
                receipts.push(MemberReceipt {
                    file_id: members[index].file_id.clone(),
                    store_id: members[index].store_id.clone(),
                    log_epoch: members[index].log_epoch.clone(),
                    heads,
                });
            }
            let vector = ReceiptVector {
                batch_id: batch_id.to_owned(),
                digest: existing.digest.clone(),
                members: receipts,
            };
            let encoded = serde_json::to_string(&vector).map_err(refuse)?;
            durable(&mut guards[0], |store| {
                store.conn.execute(
                    "UPDATE participant_journal SET state='published', receipts=?2 \
                     WHERE batch_id=?1 AND state='committed'",
                    rusqlite::params![batch_id, encoded],
                )?;
                Ok(())
            })?;
            crash(CrashPoint::AfterPublish)?;
            release(guards, batch_id)?;
            Ok(Recovery::Published(vector))
        }
        "published" => {
            let vector: ReceiptVector = serde_json::from_str(
                existing
                    .receipts
                    .as_deref()
                    .ok_or_else(|| refuse("published journal lacks receipts"))?,
            )
            .map_err(|_| refuse("damaged published receipts"))?;
            release(guards, batch_id)?;
            Ok(Recovery::Published(vector))
        }
        _ => Err(refuse("unknown journal state")),
    }
}

/// Apply one verified intent: facts and the `applied` receipt in ONE local
/// transaction. The expected head must be unchanged.
fn apply(
    store: &mut Store,
    batch_id: &str,
    row: &IntentRow,
    payload: &Decoded,
) -> Result<Vec<(String, i64)>> {
    if store.latest_tx_id()? != row.expected_head {
        return Err(refuse(
            "participant head moved after preparation; participants stay fenced",
        ));
    }
    let native = payload
        .graphs
        .iter()
        .map(|(graph, datums)| {
            let id = store
                .committed_graph_id(graph)?
                .ok_or_else(|| refuse("prepared graph is absent or not committed"))?;
            Ok((id, datums.clone()))
        })
        .collect::<Result<Vec<_>>>()?;
    store.ownership_operation(|store| {
        store
            .conn
            .execute_batch("SAVEPOINT quipu_participant_apply")?;
        let outcome = (|| {
            let txs = store.transact_graph_batches_tx(
                &native,
                &payload.timestamp,
                payload.actor.as_deref(),
                payload.source.as_deref(),
            )?;
            let heads: Vec<(String, i64)> = payload
                .graphs
                .iter()
                .zip(txs)
                .map(|((graph, _), (_, tx))| (graph.clone(), tx))
                .collect();
            let encoded = serde_json::to_string(&heads).map_err(refuse)?;
            let changed = store.conn.execute(
                "UPDATE participant_intents SET state='applied', heads=?2 \
                 WHERE batch_id=?1 AND state='prepared'",
                rusqlite::params![batch_id, encoded],
            )?;
            if changed != 1 {
                return Err(refuse("intent changed during apply"));
            }
            Ok(heads)
        })();
        match outcome {
            Ok(heads) => {
                crate::store::release::release(&store.conn, "quipu_participant_apply")?;
                Ok(heads)
            }
            Err(error) => {
                crate::store::release::rollback_release(&store.conn, "quipu_participant_apply")?;
                store.read_model.borrow_mut().clear();
                Err(error)
            }
        }
    })
}

/// After PUBLISHED: clear every participant's fence for this batch.
fn release(guards: &mut [Guard<'_>], batch_id: &str) -> Result<()> {
    for store in guards.iter_mut() {
        durable(store, |store| {
            store.conn.execute(
                "UPDATE participant_intents SET state='published' \
                 WHERE batch_id=?1 AND state='applied'",
                [batch_id],
            )?;
            Ok(())
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "participant_commit_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "participant_kill_tests.rs"]
mod kill_tests;
