//! Protected per-lineage authority journal: the P1 durable-role file
//! (docs/design/ownership-handoff.md, "Durable authority and rollback limits";
//! plan P1; aegis-s24.8).
//!
//! The journal lives OUTSIDE the database, its sidecar and any transferable
//! backup, so restoring an old database cannot erase a retirement. It holds one
//! `ownership_handoff_model` `Journal` record (Active, Retiring, Retired or
//! Pending), strictly encoded by the model's codec, followed by a sha256 trailer.
//!
//! Reading never guesses. A missing, unreadable, oversized, malformed,
//! wrong-kind, invalid or checksum-failing file is `Unknown`, never legacy or
//! inactive. Writing is an atomic replace: a sibling temporary file, fsync,
//! rename and a directory fsync. Advancing takes an exclusive lock on a sibling
//! lock file, re-reads the current record, requires it to equal what the caller
//! expected, and admits only the local transitions the design allows:
//! Active -> Retiring (same source), Retiring -> Retired (same request) and
//! Pending -> Active (the expected successor). Nothing leaves Retired, and
//! nothing un-retires.
//!
//! Durability, not rollback protection: a whole-host rollback that restores
//! this file too cannot be seen from its bytes; the startup witness covers that
//! and is a later slice. There is deliberately no public enrollment path. The
//! commit fence's generation pin calls `admits`; nothing in production sets
//! a pin yet, and the startup adapter is a later slice.
#![cfg_attr(not(test), allow(dead_code))]

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::error::{Error, Result};
use crate::ownership_handoff_model::{
    Authority, Journal, MAX_RECORD_BYTES, Payload, RECORD_VERSION, Record,
};

const TRAILER: &str = "\nsha256:";

/// What a read established. `Unknown` is never treated as legacy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum JournalState {
    // Boxed: a Journal record is far larger than the Unknown reason.
    Known(Box<Journal>),
    Unknown(String),
}

fn unknown(why: impl Into<String>) -> JournalState {
    JournalState::Unknown(why.into())
}

fn refused(why: impl Into<String>) -> Error {
    Error::PolicyDenied(format!("ownership journal: {}", why.into()))
}

/// Read the journal at `path`.
pub(super) fn read(path: &Path) -> JournalState {
    let mut bytes = Vec::new();
    let limit = (MAX_RECORD_BYTES + TRAILER.len() + 64) as u64;
    match File::open(path) {
        Ok(file) => {
            if let Err(e) = file.take(limit + 1).read_to_end(&mut bytes) {
                return unknown(format!("unreadable: {e}"));
            }
        }
        Err(e) => return unknown(format!("missing or unreadable: {e}")),
    }
    if bytes.len() as u64 > limit {
        return unknown("oversized");
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return unknown("not UTF-8");
    };
    let Some((body, trailer)) = text.rsplit_once(TRAILER) else {
        return unknown("no checksum trailer");
    };
    if trailer.trim_end_matches('\n') != hex::encode(Sha256::digest(body.as_bytes())) {
        return unknown("checksum mismatch");
    }
    match Record::decode(body.as_bytes()) {
        Ok(Record {
            payload: Payload::Journal { journal },
            ..
        }) => JournalState::Known(Box::new(journal)),
        Ok(_) => unknown("not a journal record"),
        Err(e) => unknown(format!("invalid record: {e}")),
    }
}

fn encode(journal: &Journal) -> Result<Vec<u8>> {
    let record = Record {
        version: RECORD_VERSION,
        payload: Payload::Journal {
            journal: journal.clone(),
        },
    };
    let body = record.encode().map_err(refused)?;
    let mut out = body.clone();
    out.extend_from_slice(TRAILER.as_bytes());
    out.extend_from_slice(hex::encode(Sha256::digest(&body)).as_bytes());
    out.push(b'\n');
    Ok(out)
}

fn sibling(path: &Path, suffix: &str) -> Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| refused("journal path has no file name"))?;
    let mut name = name.to_os_string();
    name.push(suffix);
    Ok(path.with_file_name(name))
}

/// Atomically replace the journal with `journal`.
pub(super) fn write_atomic(path: &Path, journal: &Journal) -> Result<()> {
    let bytes = encode(journal)?;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| refused("journal path has no directory"))?;
    let staged = sibling(path, ".staged")?;
    let io = |e: std::io::Error| refused(format!("write failed: {e}"));
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&staged)
            .map_err(io)?;
        file.write_all(&bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
    }
    std::fs::rename(&staged, path).map_err(io)?;
    File::open(dir).and_then(|d| d.sync_all()).map_err(io)?;
    Ok(())
}

/// Whether `next` may follow `current` in this owner's local journal.
fn legal(current: &Journal, next: &Journal) -> std::result::Result<(), &'static str> {
    match (current, next) {
        (Journal::Active { authority }, Journal::Retiring { request }) => {
            if request.source == *authority {
                Ok(())
            } else {
                Err("retirement must name the active authority as its source")
            }
        }
        (Journal::Retiring { request }, Journal::Retired { certificate }) => {
            if certificate.request == *request {
                Ok(())
            } else {
                Err("retirement certificate must complete the reserved request")
            }
        }
        (Journal::Pending { successor, .. }, Journal::Active { authority }) => {
            if authority == successor {
                Ok(())
            } else {
                Err("activation must install the pending successor")
            }
        }
        (Journal::Retired { .. }, _) => Err("a retired owner cannot change role"),
        (Journal::Retiring { .. }, Journal::Active { .. }) => Err("retirement cannot be undone"),
        _ => Err("transition is not a local journal step"),
    }
}

/// Advance the journal from `expected` to `next` under an exclusive lock,
/// refusing if the stored role is anything other than `expected`.
pub(super) fn advance(path: &Path, expected: &Journal, next: &Journal) -> Result<()> {
    let lock_path = sibling(path, ".lock")?;
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| refused(format!("lock unavailable: {e}")))?;
    lock.lock()
        .map_err(|e| refused(format!("lock unavailable: {e}")))?;
    let result = match read(path) {
        JournalState::Known(current) if *current == *expected => legal(&current, next)
            .map_err(refused)
            .and_then(|()| write_atomic(path, next)),
        JournalState::Known(_) => Err(refused("stored role is not the expected one")),
        JournalState::Unknown(why) => Err(refused(format!("current role is UNKNOWN: {why}"))),
    };
    drop(lock);
    result
}

/// Whether the journal at `path` still admits a handle pinned to `pinned`:
/// only `Active` at exactly that authority tuple does. Every other state
/// refuses with its role named, never as legacy or inactive.
pub(super) fn admits(path: &Path, pinned: &Authority) -> Result<()> {
    let role = match read(path) {
        JournalState::Unknown(why) => return Err(refused(format!("role is UNKNOWN: {why}"))),
        JournalState::Known(journal) => match *journal {
            Journal::Active { ref authority } if authority == pinned => return Ok(()),
            Journal::Active { ref authority } => format!(
                "ACTIVE at generation {} epoch {}, not this handle's generation {} epoch {}",
                authority.generation, authority.epoch, pinned.generation, pinned.epoch
            ),
            Journal::Retiring { .. } => "RETIRING".into(),
            Journal::Retired { .. } => "RETIRED".into(),
            Journal::Pending { .. } => "PENDING".into(),
        },
    };
    Err(refused(format!(
        "role is {role}; this handle's admission is revoked"
    )))
}

/// Create a journal for a new lineage. Crate-private on purpose: there is no
/// public enrollment path, and the startup adapter that would call this is not
/// built yet. Refuses if any journal file already exists.
pub(super) fn create(path: &Path, initial: &Journal) -> Result<()> {
    if !matches!(initial, Journal::Active { .. }) {
        return Err(refused("a new journal starts Active"));
    }
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(refused("a journal already exists"));
    }
    write_atomic(path, initial)
}

#[cfg(test)]
#[path = "ownership_journal_tests.rs"]
mod tests;
