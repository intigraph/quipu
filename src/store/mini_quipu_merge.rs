//! Mini-Quipu slice M3 (docs/design/mini-quipu.md §2.3): merge back IS fork
//! promote (U2). The only difference from a same-store fork is where `theirs`
//! is read: the mini-Quipu file, through a read-only connection. The value
//! rule, the plan and the write are the fork path's own: `Store::fork_merge`
//! over `merge3::merge_slots` and `merge_core::plan_merge` (or `select` for
//! the partial modes of oracle §6), then `receive` and `decide` through the
//! ordinary write gate in ONE savepoint on the home owner. The merge id is
//! `merge_core::merge_id` on the `fork` path.
//!
//! Admission runs in the design's order and every refusal writes nothing:
//! 1 registry open and epochs (`foreign-epoch`); 2 base rebuildable at the
//! cut (`base-unavailable`) and equal to the digest (`base-mismatch`);
//! 3 current home write authority (the owner's fence, then the write gate);
//! 4 one home owner under the current catalog generation (X4); 5 plan, and
//! the write gate under CURRENT home shapes, no exemption; 6 partial modes.
use std::path::Path;

use rusqlite::{OptionalExtension, params};

use super::{
    GraphSetOwner, LastMerge, MergeBase, MiniForkEntry, Owner, base_digest, live_digest,
    member_journal,
};
use crate::{
    Error, Result, Store, graph_catalog::GraphSetCatalog, merge_core::selection::Mode,
    merge3::DecisionRecord, store::forks_merge::ForkResolutions,
};

const MERGES: &str = "CREATE TABLE IF NOT EXISTS mini_quipu_merges (
    merge_id   TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    incoming   TEXT NOT NULL,
    decisions  TEXT NOT NULL,
    mode       TEXT NOT NULL,
    resolved   TEXT NOT NULL,
    receipt    TEXT NOT NULL,
    UNIQUE (name, incoming, decisions, mode, resolved)
)";
const SAVEPOINT: &str = "quipu_mini_merge";

/// Oracle §6 partial modes; `Atomic` by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MergeMode {
    /// Any held slot holds the whole merge.
    #[default]
    Atomic,
    /// A held slot holds every slot of its entity.
    PartialEntity,
    /// Only the held slot is held.
    PartialSlot,
}

impl MergeMode {
    fn code(self) -> &'static str {
        match self {
            Self::Atomic => "atomic",
            Self::PartialEntity => "partial-entity",
            Self::PartialSlot => "partial-slot",
        }
    }
}

/// A digest of the resolutions themselves, so the R7 key never trusts the
/// caller's decisions hash alone (the fork promote contract keeps that hash).
fn resolved_digest(resolutions: Option<&ForkResolutions>) -> String {
    let mut rows: Vec<_> = resolutions
        .into_iter()
        .flatten()
        .map(|(&(e, a), values)| {
            let mut values: Vec<String> = values
                .iter()
                .map(|v| v.to_bytes().iter().map(|b| format!("{b:02x}")).collect())
                .collect();
            values.sort();
            (e, a, values)
        })
        .collect();
    rows.sort();
    crate::share::sha256(serde_json::json!(rows).to_string().as_bytes())
}

impl From<MergeMode> for Mode {
    fn from(m: MergeMode) -> Self {
        match m {
            MergeMode::Atomic => Mode::Atomic,
            MergeMode::PartialEntity => Mode::PartialEntity,
            MergeMode::PartialSlot => Mode::PartialSlot,
        }
    }
}

/// The named admission refusals of this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeRefusal {
    /// The registry entry is retired or absent.
    RegistryClosed,
    /// The file or the home owner is not the identity and epoch recorded.
    ForeignEpoch,
    /// Home cannot rebuild the base as of the recorded cut.
    BaseUnavailable,
    /// The rebuilt base differs from the recorded digest.
    BaseMismatch,
    /// Writable members moved off the recorded home owner or generation.
    X4MovedOwner,
    /// The last merge still holds slots back. Apply its remainder (the same
    /// incoming) before merging a new head; otherwise the new base would
    /// equal the mini's held values and silently drop the holds. Once the
    /// mini-Quipu has written past that incoming no remainder can be made,
    /// so the way out is a re-fork.
    RemainderPending,
    /// The entry records a merged head but no last merge: it was advanced by
    /// M3 (#1318), whose home cut already folds in home-only changes, held
    /// slots and decisions. Planning on it would land the mini's unchanged
    /// values as one-sided changes, so it refuses; re-fork from current home.
    SecondMerge,
    /// The caller's decisions hash already names different resolutions.
    DecisionsHashReused,
}

impl MergeRefusal {
    /// The stable reason code.
    pub fn code(self) -> &'static str {
        match self {
            Self::RegistryClosed => "registry-closed",
            Self::ForeignEpoch => "foreign-epoch",
            Self::BaseUnavailable => "base-unavailable",
            Self::BaseMismatch => "base-mismatch",
            Self::X4MovedOwner => "x4-multi-file",
            Self::RemainderPending => "remainder-pending",
            Self::SecondMerge => "second-merge",
            Self::DecisionsHashReused => "decisions-hash-reused",
        }
    }
}

fn refused(reason: MergeRefusal, detail: &str) -> Error {
    Error::InvalidValue(format!(
        "mini-Quipu merge refused ({}): {detail}; nothing was written",
        reason.code()
    ))
}

/// One merge-back request. `decided` is the reconcile flow's resolutions
/// (keyed by home term ids), their hash and the reviewer.
pub struct MergeRequest<'a> {
    /// Registry entry name.
    pub name: &'a str,
    /// Partial mode, oracle §6.
    pub mode: MergeMode,
    /// Decided conflicting slots, decisions hash, reviewer.
    pub decided: Option<(&'a ForkResolutions, &'a str, &'a str)>,
    /// The merge instant `Td`.
    pub timestamp: &'a str,
    /// Home applier.
    pub actor: Option<&'a str>,
}

/// What a merge committed, or committed earlier for the same input.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MergeReceipt {
    /// `merge_core::merge_id` on the `fork` path.
    pub merge_id: String,
    /// `store_id@log_epoch#local_tx` of the merged mini-Quipu head.
    pub incoming: String,
    /// Home `receive` transactions, one per member written (`0` for none).
    pub receive_tx: Vec<i64>,
    /// Home `decide` transactions, one per member written (`0` for none).
    pub decide_tx: Vec<i64>,
    /// Slots a partial mode held back.
    pub held: usize,
}

/// The outcome of a merge back.
#[derive(Debug)]
pub enum MergeBack {
    /// Written through the home write gate in one savepoint.
    Merged(MergeReceipt),
    /// R7: the same input, mode and resolutions were merged before.
    AlreadyApplied(MergeReceipt),
    /// Atomic mode and undecided conflicts. Nothing was written.
    Conflicts(Vec<DecisionRecord>),
    /// The current home shapes refused. Nothing was written.
    #[cfg(feature = "shacl")]
    Refused(crate::shacl::ValidationFeedback),
}

#[path = "mini_quipu_merge_plan.rs"]
mod plan;
use plan::{open_theirs, plan_member, verified_mini};

/// Step 4: every writable member is still on the recorded home owner and
/// generation (X4).
fn x4(catalog: &GraphSetCatalog, entry: &MiniForkEntry) -> Result<()> {
    let selected: Vec<&str> = entry.writable.iter().map(String::as_str).collect();
    let groups = catalog.plan_read(&selected).map_err(|_| {
        refused(
            MergeRefusal::X4MovedOwner,
            "a writable member left the catalog",
        )
    })?;
    if groups.len() != 1
        || groups[0].owner.store_id != entry.base.store_id
        || groups[0].owner.generation != entry.base.owner_generation
    {
        return Err(refused(
            MergeRefusal::X4MovedOwner,
            "writable members are no longer on the recorded home owner and generation",
        ));
    }
    Ok(())
}

/// Steps 1 and 2, reading only. Returns the mini file.
fn admit(home: &Store, path: &Path, entry: &MiniForkEntry) -> Result<Store> {
    let open: Option<String> = home
        .conn
        .query_row(
            "SELECT status FROM mini_quipu_forks WHERE name = ?1",
            [&entry.name],
            |r| r.get(0),
        )
        .optional()?;
    if open.as_deref() != Some("open") {
        return Err(refused(
            MergeRefusal::RegistryClosed,
            "the registry entry is not open",
        ));
    }
    if home.store_id()? != entry.base.store_id || home.log_epoch()? != entry.base.log_epoch {
        return Err(refused(
            MergeRefusal::ForeignEpoch,
            "the home owner is not the identity and epoch of the base cut",
        ));
    }
    let mini = open_theirs(path, entry)?;
    let cut = entry.base.local_tx;
    if cut < 0 || cut > home.latest_tx_id()? {
        return Err(refused(
            MergeRefusal::BaseUnavailable,
            "the base cut is not in home history",
        ));
    }
    let journals = entry
        .writable
        .iter()
        .map(|g| member_journal(home, cut, g))
        .collect::<Result<Vec<_>>>()
        .map_err(|_| {
            refused(
                MergeRefusal::BaseUnavailable,
                "home cannot rebuild the base",
            )
        })?;
    if base_digest(&journals)? != entry.base_digest {
        return Err(refused(
            MergeRefusal::BaseMismatch,
            "the rebuilt base differs from the recorded digest",
        ));
    }
    Ok(mini)
}

fn merge(
    home: &mut Store,
    catalog: &GraphSetCatalog,
    path: &Path,
    req: &MergeRequest<'_>,
) -> Result<MergeBack> {
    let entry = super::mini_fork_entry(home, req.name)?
        .ok_or_else(|| refused(MergeRefusal::RegistryClosed, "no such registry entry"))?;
    let mini = admit(home, path, &entry)?;
    let incoming = format!(
        "{}@{}#{}",
        entry.store_id,
        entry.log_epoch,
        mini.latest_tx_id()?
    );
    let decisions = req
        .decided
        .map_or_else(|| crate::share::sha256(b"[]"), |d| d.1.to_owned());
    let mode = req.mode.code();
    let resolved = resolved_digest(req.decided.map(|d| d.0));
    if super::has_table(home, "mini_quipu_merges")?
        && let Some(r) = home
            .conn
            .query_row(
                "SELECT receipt FROM mini_quipu_merges WHERE name=?1 AND incoming=?2 \
                 AND decisions=?3 AND mode=?4 AND resolved=?5",
                params![req.name, incoming, decisions, mode, resolved],
                |r| r.get::<_, String>(0),
            )
            .optional()?
    {
        let receipt = serde_json::from_str(&r).map_err(|e| Error::Serialization(e.to_string()))?;
        return Ok(MergeBack::AlreadyApplied(receipt));
    }
    let (base, after) = later_base(&mini, &entry, &incoming)?;
    let base_id = match &base {
        MergeBase::Home => format!(
            "{}@{}#{}:{}",
            entry.base.store_id, entry.base.log_epoch, entry.base.local_tx, entry.base_digest
        ),
        MergeBase::Mini { head, digest } => {
            format!("{}@{}#{head}:{digest}", entry.store_id, entry.log_epoch)
        }
    };
    // Atomic keeps the fork path's exact id; a partial mode is part of it.
    let keyed = if req.mode == MergeMode::Atomic {
        decisions.clone()
    } else {
        format!("{decisions};mode={mode}")
    };
    let merge_id = crate::merge_core::merge_id("fork", &incoming, &base_id, &keyed);
    home.ownership_operation(|home| {
        // Step 3: current home write authority. The fence is checked here,
        // and every gate of the write pipeline binds `receive` and `decide`.
        home.ensure_ownership_writable()?;
        // Step 4 after step 3 (design order): one home owner, current generation.
        x4(catalog, &entry)?;
        home.conn.execute_batch(&format!("SAVEPOINT {SAVEPOINT}"))?;
        let key = [
            incoming.as_str(),
            decisions.as_str(),
            mode,
            resolved.as_str(),
        ];
        let at = Later {
            base: &base,
            after: after.as_deref(),
        };
        match write(home, &mini, &entry, req, &merge_id, &at, key) {
            Ok(MergeBack::Merged(r)) => {
                crate::store::release::release(&home.conn, SAVEPOINT)?;
                Ok(MergeBack::Merged(r))
            }
            other => {
                let _ = crate::store::release::rollback_release(&home.conn, SAVEPOINT);
                other
            }
        }
    })
}

/// Steps 5-6 inside the savepoint; anything but `Merged` is rolled back,
/// including the dictionary terms the plan interned.
fn write(
    home: &mut Store,
    mini: &Store,
    entry: &MiniForkEntry,
    req: &MergeRequest<'_>,
    merge_id: &str,
    at: &Later<'_>,
    [incoming, decisions, mode, resolved]: [&str; 4],
) -> Result<MergeBack> {
    let cut = entry.base.local_tx;
    let mut plans = Vec::new();
    for graph in &entry.writable {
        let (g, plan) = plan_member(
            home,
            mini,
            graph,
            (at.base, cut),
            req.decided.map(|d| d.0),
            req.mode.into(),
            Some(req.timestamp),
        )?;
        if req.mode == MergeMode::Atomic && !plan.conflicts.is_empty() {
            return Ok(MergeBack::Conflicts(plan.conflicts));
        }
        #[cfg(feature = "shacl")]
        if let Some(feedback) = gate(home, g, &plan)? {
            return Ok(MergeBack::Refused(feedback));
        }
        plans.push((g, plan));
    }
    let mut source = format!(
        "mini-quipu-merge:{};merge={merge_id};incoming={incoming}",
        req.name
    );
    if let Some((_, sha, reviewer)) = req.decided {
        source.push_str(&format!(";decisions={sha};reviewer={reviewer}"));
    }
    // Oracle §6 sequencing: a later merge or a remainder names the earlier.
    if let Some(earlier) = at.after {
        source.push_str(&format!(";after={earlier}"));
    }
    let mut receipt = MergeReceipt {
        merge_id: merge_id.to_owned(),
        incoming: incoming.to_owned(),
        receive_tx: Vec::new(),
        decide_tx: Vec::new(),
        held: plans.iter().map(|(_, p)| p.held).sum(),
    };
    for (step, decide) in [("receive", false), ("decide", true)] {
        for (g, plan) in &plans {
            let datums = if decide { &plan.decide } else { &plan.receive };
            #[cfg(not(target_arch = "wasm32"))]
            crate::merge_plan_gate::datums(home, &[datums], None)?;
            let tx = if datums.is_empty() {
                0
            } else {
                let src = format!("{source};step={step}");
                home.transact_to_graph(datums, req.timestamp, req.actor, Some(&src), *g)?
            };
            if decide {
                &mut receipt.decide_tx
            } else {
                &mut receipt.receive_tx
            }
            .push(tx);
        }
    }
    advance(home, mini, entry, &receipt, at.base)?;
    home.conn.execute_batch(MERGES)?;
    let taken: Option<String> = home
        .conn
        .query_row(
            "SELECT resolved FROM mini_quipu_merges WHERE merge_id = ?1",
            [merge_id],
            |r| r.get(0),
        )
        .optional()?;
    if taken.is_some_and(|other| other != resolved) {
        return Err(refused(
            MergeRefusal::DecisionsHashReused,
            "this decisions hash already named different resolutions",
        ));
    }
    home.conn.execute(
        "INSERT INTO mini_quipu_merges (merge_id, name, incoming, decisions, mode, resolved, \
         receipt) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            merge_id,
            req.name,
            incoming,
            decisions,
            mode,
            resolved,
            serde_json::to_string(&receipt).map_err(|e| Error::Serialization(e.to_string()))?
        ],
    )?;
    Ok(MergeBack::Merged(receipt))
}

/// The design's last paragraph, as a merge base rather than a home cut: a
/// merge that wrote records its head, the base it used (for its remainder)
/// and the digest of the mini-Quipu's live members at that head (the base of
/// the next merge of a new head). The registry's home cut never moves, so
/// home-only changes, held slots and decisions are never folded into a base
/// the mini did not see.
fn advance(
    home: &Store,
    mini: &Store,
    entry: &MiniForkEntry,
    receipt: &MergeReceipt,
    base: &MergeBase,
) -> Result<()> {
    // A remainder of the last merge is recorded even when it wrote nothing
    // (every held slot decided to ours): otherwise its held count stays
    // stale and every new head refuses `remainder-pending`. It keeps the
    // digest recorded at that head, which `later_base` already checks.
    let remainder = entry
        .last_merge
        .as_ref()
        .filter(|last| last.incoming == receipt.incoming);
    let wrote = receipt
        .decide_tx
        .iter()
        .chain(&receipt.receive_tx)
        .any(|tx| *tx > 0);
    if !wrote && remainder.is_none() {
        return Ok(());
    }
    let head = incoming_head(&receipt.incoming)?;
    let head_digest = match remainder {
        Some(last) => last.head_digest.clone(),
        None => live_digest(mini, head, &entry.writable)?,
    };
    let mut next = entry.clone();
    next.merged_head = Some(head);
    next.last_merge = Some(LastMerge {
        merge_id: receipt.merge_id.clone(),
        incoming: receipt.incoming.clone(),
        held: receipt.held,
        base: base.clone(),
        head_digest,
    });
    home.conn.execute(
        "UPDATE mini_quipu_forks SET entry = ?1 WHERE name = ?2",
        params![
            serde_json::to_string(&next).map_err(|e| Error::Serialization(e.to_string()))?,
            entry.name
        ],
    )?;
    Ok(())
}

fn incoming_head(incoming: &str) -> Result<i64> {
    incoming
        .rsplit_once('#')
        .and_then(|(_, tx)| tx.parse().ok())
        .ok_or_else(|| Error::InvalidValue(format!("mini merge: bad incoming {incoming}")))
}

/// Where a merge plans from, and which earlier merge it names.
struct Later<'a> {
    base: &'a MergeBase,
    after: Option<&'a str>,
}

/// Step 2 for a later merge. The first merge plans against the home cut. A
/// remainder (the same incoming as the last merge) reuses that merge's base.
/// A new head plans against the mini-Quipu's own state at the last merged
/// head, checked against the digest home recorded (`base-mismatch` if the
/// history was rewritten, `base-unavailable` if it cannot be read), and
/// refuses while the last merge still holds slots (`remainder-pending`).
fn later_base(
    mini: &Store,
    entry: &MiniForkEntry,
    incoming: &str,
) -> Result<(MergeBase, Option<String>)> {
    let (head, last) = match (entry.merged_head, entry.last_merge.as_ref()) {
        (None, _) => return Ok((MergeBase::Home, None)),
        (Some(head), Some(last)) => (head, last),
        (Some(head), None) => {
            return Err(refused(
                MergeRefusal::SecondMerge,
                &format!(
                    "the entry was advanced to mini head {head} with no recorded \
                     last merge (an M3-era home cut); re-fork from current home"
                ),
            ));
        }
    };
    if last.incoming == incoming {
        // A remainder reuses the earlier base. A mini base is re-verified:
        // the history may have been rewritten since that merge.
        if let MergeBase::Mini { head, digest } = &last.base {
            verified_mini(mini, entry, *head, digest)?;
        }
        return Ok((last.base.clone(), Some(last.merge_id.clone())));
    }
    if last.held > 0 {
        return Err(refused(
            MergeRefusal::RemainderPending,
            &format!(
                "merge {} still holds {} slot(s); apply its remainder (the same \
                 incoming) first, or re-fork if the mini-Quipu has written since",
                last.merge_id, last.held
            ),
        ));
    }
    let digest = verified_mini(mini, entry, head, &last.head_digest)?;
    Ok((
        MergeBase::Mini { head, digest },
        Some(last.merge_id.clone()),
    ))
}

/// Step 5's gate: the CURRENT home reject-mode shapes over the asserted
/// delta, and the deletion gate over the closes, as fork promote applies.
#[cfg(feature = "shacl")]
fn gate(
    home: &Store,
    g: i64,
    plan: &crate::store::forks_merge::ForkMerge,
) -> Result<Option<crate::shacl::ValidationFeedback>> {
    let nt = |facts: &[crate::types::Fact]| -> Result<String> {
        let bytes = crate::rdf::serialize_facts(home, facts, oxrdfio::RdfFormat::NTriples)?;
        String::from_utf8(bytes).map_err(|e| Error::InvalidValue(format!("mini merge: {e}")))
    };
    if !plan.added.is_empty()
        && let Some(shapes) = home.get_combined_shapes()?
    {
        let split = crate::shacl::split_shapes_by_policy(&shapes);
        let feedback = crate::shacl_context::validate_with_store_context(
            home,
            &split.reject,
            &nt(&plan.added)?,
        )?;
        if !feedback.conforms {
            return Ok(Some(feedback));
        }
    }
    if !plan.removed.is_empty() {
        return home.retraction_refusal_nt(&nt(&plan.removed)?, &nt(&plan.added)?, g, true);
    }
    Ok(None)
}

/// Merge a mini-Quipu whose writable members are on the legacy file.
pub fn merge_on_legacy(
    catalog: &GraphSetCatalog,
    legacy: &mut Store,
    path: &Path,
    req: &MergeRequest<'_>,
) -> Result<MergeBack> {
    merge(legacy, catalog, path, req)
}

impl GraphSetOwner {
    /// Merge a mini-Quipu back through this owner's one writer.
    pub fn merge_mini_quipu(&self, path: &Path, req: &MergeRequest<'_>) -> Result<MergeBack> {
        let _admission = self.owner.admit()?;
        let mut store = self.owner.writer.owned()?;
        self.owner.placement.verify()?;
        Owner::check_identity(self.owner.catalog_route(), &store)?;
        merge(&mut store, &self.catalog, path, req)
    }
}

#[cfg(test)]
#[path = "mini_quipu_merge_tests.rs"]
mod tests;
