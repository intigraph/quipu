//! Owner retirement to legacy ROOT (aegis-4qv.4, dearing DESIGN RULINGS
//! Q1-Q3): the first production caller of the witness transitions. A reversed
//! pilot owner is RETIRED through the ownership handoff protocol, not merely
//! unrouted:
//!
//!   1. refuse unless the configured profile equals the witness's profile,
//!      the destination is the legacy ROOT placement of the named legacy
//!      database, and witness head and journal agree on the exact tuple;
//!   2. witness `reserve` (create-only), then journal Active -> Retiring;
//!   3. witness `retire` with the caller's sealed cut, then journal
//!      Retiring -> Retired.
//!
//! There is NO `activate`: legacy ROOT does not seek admission through the
//! witness, and `WitnessHead::Retired` is an exact completed retirement, not a
//! replacement-owner grant. The Retired record is terminal for this tuple; a
//! later re-enrollment is a new genesis with a new store tuple.
//!
//! UNSIGNED: the request names an existing operator and the configured
//! profile, and the transition's authority is the witness's create-only
//! conditional write plus that profile match, the same basis as genesis. No
//! signature is produced or claimed.
//!
//! Any `Reply::Unknown` stops with an UNKNOWN refusal: no retry loop and no
//! fallback. A re-run resumes from whatever the witness and journal durably
//! hold, because the request is a pure function of the journal's tuple and
//! the inputs, and the witness resolves an exact retry by readback.
use std::path::Path;

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::ownership_admission::database;
use super::ownership_enroll::refused;
use super::ownership_inventory::Database;
use super::ownership_journal::{self, JournalState};
use crate::error::Result;
use crate::ownership_handoff_model::witness::objects::Objects;
use crate::ownership_handoff_model::witness::{AuthorityWitness, ObjectWitness, Reply};
use crate::ownership_handoff_model::{Certificate, Cut, Journal, Placement, Request, WitnessHead};

/// Domain tag for a legacy ROOT native binding. Deliberately distinct from
/// the enrollment tag, so a ROOT placement is never an enrolled placement.
const LEGACY_ROOT_TAG: &[u8] = b"quipu-legacy-root-native/v1\0";

/// The only witness attempt this caller drives. A lineage with an earlier
/// abandoned attempt is refused by the witness as a conflict, fail-closed.
const ATTEMPT: u64 = 0;

pub(crate) struct RetireRequest<'a, B: Objects> {
    /// The enrolled owner's journal (Active at the tuple being retired).
    pub(crate) journal_path: &'a Path,
    /// Profile named by the operator's configuration; must equal the witness.
    pub(crate) profile: &'a str,
    /// Existing operator principal; never minted here.
    pub(crate) operator: &'a str,
    /// Destination; must be exactly [`legacy_root_placement`] of `legacy_db`.
    pub(crate) destination: Placement,
    /// The legacy ROOT database the graph was reversed into.
    pub(crate) legacy_db: &'a Path,
    /// The sealed source cut (last transaction, fact and artifact digests).
    pub(crate) cut: Cut,
    pub(crate) witness_profile: &'a str,
    pub(crate) objects: &'a B,
}

/// What retirement established. Deterministic for one journal and request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct RetireReceipt {
    pub(crate) store_id: String,
    pub(crate) generation: u64,
    pub(crate) operation: String,
    /// `RETIRED` on success; the only state this receipt is ever built for.
    pub(crate) witness_state: &'static str,
    pub(crate) journal_state: &'static str,
    pub(crate) certificate_sha256: String,
    /// Always false: this is a witness-recorded transition, not a signature.
    pub(crate) signed: bool,
}

/// The legacy ROOT placement for one legacy database binding.
pub(crate) fn legacy_root_placement(principal: &str, db: &Database) -> Placement {
    Placement {
        principal: principal.into(),
        native: legacy_root_native(db),
    }
}

fn legacy_root_native(db: &Database) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(LEGACY_ROOT_TAG);
    h.update(db.path.as_bytes());
    h.update(b"\0");
    h.update(db.device.to_be_bytes());
    h.update(db.inode.to_be_bytes());
    h.finalize().into()
}

/// The deterministic request for retiring `source`'s tuple to `destination`.
fn plan(req: &RetireRequest<'_, impl Objects>, journal: &Journal) -> Result<Request> {
    let source = match journal {
        Journal::Active { authority } => authority.clone(),
        Journal::Retiring { request } => request.source.clone(),
        Journal::Retired { certificate } => certificate.request.source.clone(),
        Journal::Pending { .. } => return Err(refused("journal is PENDING, not an owner")),
    };
    let successor_generation = source
        .generation
        .checked_add(1)
        .ok_or_else(|| refused("generation exhausted"))?;
    let request = Request {
        operation: format!(
            "retire-to-legacy-root/{}/{}/{}",
            source.store_id, source.generation, source.epoch
        ),
        operator: req.operator.into(),
        source,
        destination: req.destination.clone(),
        successor_generation,
        profile: req.profile.into(),
    };
    request
        .validate()
        .map_err(|e| refused(format!("retirement request: {e:?}")))?;
    Ok(request)
}

fn check_destination(req: &RetireRequest<'_, impl Objects>) -> Result<()> {
    let (_, db) = database(req.legacy_db)?;
    if req.destination.native == super::ownership_enroll::native_digest(&db) {
        return Err(refused(
            "destination uses the enrollment domain tag; a legacy ROOT placement never does",
        ));
    }
    if req.destination.native != legacy_root_native(&db) {
        return Err(refused(
            "destination is not the legacy ROOT placement of the legacy database",
        ));
    }
    Ok(())
}

fn unknown(step: &str) -> crate::error::Error {
    refused(format!(
        "witness {step} is UNKNOWN; stopped without retry, re-run to resume"
    ))
}

/// Local preflight shared by the dry run and execution: profile match,
/// legacy ROOT destination, and the planned request from the journal.
fn preflight<B: Objects>(req: &RetireRequest<'_, B>) -> Result<(Journal, Request)> {
    if req.profile != req.witness_profile {
        return Err(refused("witness profile differs from the configuration"));
    }
    check_destination(req)?;
    let journal = match ownership_journal::read(req.journal_path) {
        JournalState::Known(j) => *j,
        JournalState::Unknown(why) => return Err(refused(format!("journal is UNKNOWN: {why}"))),
    };
    let request = plan(req, &journal)?;
    // A journal already past Active must hold exactly this request.
    let same = match &journal {
        Journal::Retiring { request: r } => *r == request,
        Journal::Retired { certificate } => certificate.request == request,
        _ => true,
    };
    if !same {
        return Err(refused(
            "journal holds a different retirement request (operator, destination or profile)",
        ));
    }
    Ok((journal, request))
}

/// Dry run: the request retirement would reserve. No witness or local write.
pub(crate) fn plan_retirement<B: Objects>(req: &RetireRequest<'_, B>) -> Result<Request> {
    Ok(preflight(req)?.1)
}

/// Retire (or resume retiring) the owner whose journal is at `journal_path`.
pub(crate) fn retire_owner<B: Objects>(req: &RetireRequest<'_, B>) -> Result<RetireReceipt> {
    let (journal, request) = preflight(req)?;
    let certificate = Certificate {
        request: request.clone(),
        cut: req.cut.clone(),
    };
    let digest = certificate
        .digest()
        .map_err(|e| refused(format!("retirement certificate: {e:?}")))?;
    let witness = ObjectWitness::attach(request.source.store_id.clone(), req.objects);
    let head = match witness.head(&request.source.store_id, 0) {
        Reply::Ok(head) => head,
        Reply::Conflict => return Err(refused("witness lineage is not this store's")),
        Reply::Unknown => return Err(unknown("head")),
    };
    // The witness and the journal must agree on exactly where we are.
    let reserved = WitnessHead::Reserved {
        request: request.clone(),
    };
    match (&head, &journal) {
        (WitnessHead::Active { authority }, Journal::Active { authority: local })
            if *authority == request.source && local == authority => {}
        (h, Journal::Active { .. } | Journal::Retiring { .. }) if *h == reserved => {}
        (
            WitnessHead::Retired { certificate: c },
            Journal::Retiring { .. } | Journal::Retired { .. },
        ) if c.request == request => {}
        _ => {
            return Err(refused(
                "witness head and journal do not both name the expected tuple",
            ));
        }
    }
    if matches!(head, WitnessHead::Active { .. }) {
        match witness.reserve(ATTEMPT, &request) {
            Reply::Ok(()) => {}
            Reply::Conflict => return Err(refused("witness reservation conflicts")),
            Reply::Unknown => return Err(unknown("reserve")),
        }
    }
    if let Journal::Active { .. } = journal {
        ownership_journal::advance(
            req.journal_path,
            &journal,
            &Journal::Retiring {
                request: request.clone(),
            },
        )?;
    }
    // The witness's own certificate, when it already holds one, is the
    // record; a different cut cannot replace it.
    let recorded = match head {
        WitnessHead::Retired { certificate: c } => c,
        _ => {
            match witness.retire(ATTEMPT, &certificate) {
                Reply::Ok(()) => {}
                Reply::Conflict => return Err(refused("witness retirement conflicts")),
                Reply::Unknown => return Err(unknown("retire")),
            }
            certificate
        }
    };
    let retired = Journal::Retired {
        certificate: recorded.clone(),
    };
    match ownership_journal::read(req.journal_path) {
        JournalState::Known(j) if *j == retired => {}
        JournalState::Known(current) => {
            ownership_journal::advance(req.journal_path, &current, &retired)?;
        }
        JournalState::Unknown(why) => return Err(refused(format!("journal is UNKNOWN: {why}"))),
    }
    let digest = if recorded.cut == req.cut {
        digest
    } else {
        recorded
            .digest()
            .map_err(|e| refused(format!("recorded certificate: {e:?}")))?
    };
    Ok(RetireReceipt {
        store_id: request.source.store_id.clone(),
        generation: request.source.generation,
        operation: request.operation,
        witness_state: "RETIRED",
        journal_state: "RETIRED",
        certificate_sha256: hex::encode(digest),
        signed: false,
    })
}

#[cfg(test)]
#[path = "ownership_retire_tests.rs"]
mod tests;
