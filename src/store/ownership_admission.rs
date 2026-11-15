//! S1: authenticated enrolled startup admission (design D3; contract in
//! docs/design/ownership-startup-boundary.md, "S1").
//!
//! [`admit`] is the only constructor of [`Admission`]. Every step is UNKNOWN
//! on doubt and any UNKNOWN means no admission: no pin, no writer, no
//! receipt claimed as success. The receipt is audit evidence, never
//! authority, and no startup reads it back as admission.
//!
//! Nothing here enrolls a store, selects a witness bucket or principal, or is
//! called by a production constructor yet.
#![cfg(not(target_arch = "wasm32"))]
#![cfg_attr(not(test), allow(dead_code))]

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::ownership_inventory::{self as inventory, Binding, Database, Decision, Marker};
use super::{ownership_journal, ownership_latch};
use crate::error::{Error, Result};
use crate::ownership_handoff_model::witness::Reply;
use crate::ownership_handoff_model::{Authority, Journal, WitnessHead};

/// Exact admissible marker schema. SQLite keeps the original CREATE text.
pub(super) const MARKER_SQL: &str = "CREATE TABLE ownership_handoff_enrollment(id INTEGER PRIMARY KEY CHECK (id = 1), binding TEXT NOT NULL)";

const RECEIPT_VERSION: u32 = 1;
const MAX_RECEIPT_BYTES: usize = 64 * 1024;

fn refused(why: impl std::fmt::Display) -> Error {
    Error::PolicyDenied(format!(
        "store handoff: enrolled startup refused: {why}; no writable handle"
    ))
}

/// A live witness read for the inventory's profile name. Implementations are
/// deployment wiring; doubt of any kind is `Unknown`. `nonce` is the fresh
/// startup nonce written into the receipt for this exact read.
pub(crate) trait WitnessSource {
    fn head_evidence(
        &self,
        profile: &str,
        store_id: &str,
        anchor: u64,
        nonce: &[u8; 32],
    ) -> Reply<(WitnessHead, Vec<(String, String)>)>;
}

/// Explicit startup configuration: no default path, search or fallback.
pub(crate) struct EnrolledStartup<'a> {
    pub(crate) inventory: &'a Path,
    pub(crate) witness: &'a dyn WitnessSource,
}

/// Proof that this process completed S1 for one database just now. Private
/// fields, no Clone/Default/Deserialize; consumed by `CommitFence::pin_admitted`.
pub(super) struct Admission {
    canonical: PathBuf,
    database: Database,
    journal: PathBuf,
    authority: Authority,
    binding: Binding,
    // Held from before the first journal read until the pin is installed.
    _latch: ownership_latch::Shared,
}

impl Admission {
    pub(super) fn database_path(&self) -> &Path {
        &self.canonical
    }
    /// Re-check after the writable connection opened: the path must still
    /// name the admitted device/inode.
    pub(super) fn same_file_as_admitted(&self) -> Result<()> {
        let (canonical, now) = database(&self.canonical)?;
        if canonical != self.canonical || now != self.database {
            return Err(refused("database file changed after admission"));
        }
        Ok(())
    }
    /// Borrow the checked pin data without releasing the startup latch.
    /// Only `admit` can construct the enclosing proof.
    pub(super) fn pin_parts(&self) -> (&Path, &Authority, &Binding) {
        (&self.journal, &self.authority, &self.binding)
    }
}

#[derive(Serialize)]
struct Receipt<'a> {
    version: u32,
    database: &'a Database,
    inventory_record_sha256: &'a str,
    journal_sha256: String,
    authority: &'a Authority,
    witness_profile: &'a str,
    witness_versions: &'a [(String, String)],
    nonce: String,
    admitted_at_unix: u64,
}

/// Read the database marker on `conn` strictly (contract "Marker storage").
pub(super) fn read_marker(conn: &Connection) -> Result<MarkerRead> {
    let objects: Vec<(String, Option<String>)> = conn
        .prepare(
            "SELECT type, sql FROM main.sqlite_master
             WHERE name='ownership_handoff_enrollment' COLLATE NOCASE",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    match objects.as_slice() {
        [] => return Ok(MarkerRead::Absent),
        [(kind, Some(sql))] if kind == "table" && sql == MARKER_SQL => {}
        _ => return Ok(MarkerRead::Unknown),
    }
    let rows: Vec<(i64, String)> = conn
        .prepare("SELECT id, binding FROM main.ownership_handoff_enrollment")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    match rows.as_slice() {
        [(1, body)] => {
            Ok(Binding::decode(body.as_bytes()).map_or(MarkerRead::Unknown, MarkerRead::Known))
        }
        _ => Ok(MarkerRead::Unknown),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MarkerRead {
    Absent,
    Known(Binding),
    Unknown,
}

#[cfg(unix)]
fn owner_only_regular(path: &Path, what: &str, deny_mask: u32) -> Result<fs::Metadata> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let meta = fs::symlink_metadata(path).map_err(|e| refused(format!("{what}: {e}")))?;
    let euid = rustix::process::geteuid().as_raw();
    if !meta.file_type().is_file()
        || meta.nlink() != 1
        || meta.uid() != euid
        || meta.permissions().mode() & deny_mask != 0
    {
        return Err(refused(format!(
            "{what} is not a single-link regular file owned by this user with protected mode"
        )));
    }
    Ok(meta)
}

#[cfg(not(unix))]
fn owner_only_regular(_: &Path, what: &str, _: u32) -> Result<fs::Metadata> {
    Err(refused(format!(
        "{what}: protected-file checks need a Unix platform"
    )))
}

/// Step 1: canonical path and device/inode, from `lstat` of the canonical
/// path. Never opens a descriptor on the database: closing any descriptor of
/// that file drops every POSIX fcntl lock this process holds on it, including
/// SQLite's, so a check must not open and close one (4qv review).
fn database(path: &Path) -> Result<(PathBuf, Database)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let canonical = path
            .canonicalize()
            .map_err(|e| refused(format!("database path: {e}")))?;
        // lstat does not follow a link substituted after canonicalization.
        let meta = std::fs::symlink_metadata(&canonical)
            .map_err(|e| refused(format!("database stat: {e}")))?;
        if !meta.is_file() || meta.nlink() != 1 {
            return Err(refused("database is not a single-link regular file"));
        }
        let text = canonical
            .to_str()
            .ok_or_else(|| refused("database path is not UTF-8"))?
            .to_owned();
        Ok((
            canonical,
            Database {
                path: text,
                device: meta.dev(),
                inode: meta.ino(),
            },
        ))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(refused("enrolled startup needs a Unix platform"))
    }
}

fn journal_bytes(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(1 << 20).read_to_end(&mut bytes))
        .map_err(|e| refused(format!("journal read: {e}")))?;
    Ok(bytes)
}

/// Run steps 1-10 of the S1 contract for `path`.
pub(super) fn admit(path: &Path, config: &EnrolledStartup<'_>) -> Result<Admission> {
    // 1. Database identity from the open file.
    let (canonical, actual) = database(path)?;

    // 2. Marker, read without migration.
    let conn = Connection::open_with_flags(
        &canonical,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let binding = match read_marker(&conn)? {
        MarkerRead::Known(binding) => binding,
        MarkerRead::Absent => return Err(refused("no enrollment marker")),
        MarkerRead::Unknown => return Err(refused("enrollment marker is UNKNOWN")),
    };
    drop(conn);

    // 3. Protected inventory, then the E1 two-sided decision.
    owner_only_regular(config.inventory, "inventory", 0o022)?;
    let inventory =
        inventory::read(config.inventory).map_err(|e| refused(format!("inventory: {e}")))?;
    if inventory::decide(Marker::Known(&binding), Some(&inventory), &actual)
        != Decision::NeedsJournalAndWitness
    {
        return Err(refused("marker and inventory do not bind this file"));
    }
    let entry = inventory
        .records
        .iter()
        .find(|e| e.store_id == binding.store_id && e.database == actual)
        .ok_or_else(|| refused("no inventory record for this file"))?;
    let record_digest = entry
        .digest()
        .map_err(|e| refused(format!("inventory record: {e}")))?;
    if record_digest != binding.inventory_digest {
        return Err(refused("marker digest differs from the inventory record"));
    }

    // 4. Protected journal, never the database or one of its sidecars.
    let journal = PathBuf::from(&entry.journal);
    for suffix in ["", "-wal", "-shm", "-journal"] {
        if journal.as_os_str() == format!("{}{suffix}", actual.path).as_str() {
            return Err(refused("journal is the database or one of its sidecars"));
        }
    }
    owner_only_regular(&journal, "journal", 0o077)?;

    // 5. The SAME shared lineage latch the commit seam takes.
    let latch =
        ownership_latch::shared(&journal).map_err(|e| refused(format!("lineage latch: {e}")))?;

    // 6. Journal role under the latch.
    let first = journal_bytes(&journal)?;
    let authority = match ownership_journal::read(&journal) {
        ownership_journal::JournalState::Known(j) => match *j {
            Journal::Active { authority } => authority,
            Journal::Retiring { .. } | Journal::Retired { .. } => {
                return Err(refused(
                    "journal is retiring or retired; only the transition path may continue",
                ));
            }
            Journal::Pending { .. } => {
                return Err(refused(
                    "journal is PENDING; only the transition path may activate",
                ));
            }
        },
        ownership_journal::JournalState::Unknown(why) => {
            return Err(refused(format!("journal is UNKNOWN: {why}")));
        }
    };
    if authority.store_id != entry.store_id
        || authority.unit != entry.unit
        || authority.generation < entry.enrolled_at_generation
    {
        return Err(refused(
            "journal authority does not belong to this inventory record",
        ));
    }

    // 7. Fresh nonce, then a live witness head anchored at the journal role.
    let mut nonce = [0u8; 32];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce)
        .map_err(|_| refused("no fresh nonce"))?;
    let (head, versions) = match config.witness.head_evidence(
        &entry.witness,
        &authority.store_id,
        authority.generation,
        &nonce,
    ) {
        Reply::Ok(evidence) => evidence,
        Reply::Conflict => return Err(refused("witness lineage conflict")),
        Reply::Unknown => return Err(refused("witness head is UNKNOWN")),
    };

    // 8. The D3 table: only Active(g) with the identical tuple admits.
    match head {
        WitnessHead::Active { authority: ref a } if *a == authority => {}
        WitnessHead::Reserved { .. } => {
            return Err(refused(
                "witness is RESERVED; only the transition path may resume",
            ));
        }
        _ => {
            return Err(refused(
                "witness is ahead of or different from this journal; non-owner",
            ));
        }
    }
    if versions.is_empty() {
        return Err(refused("witness returned no object versions"));
    }

    // 9. Re-read the journal under the same latch.
    if journal_bytes(&journal)? != first {
        return Err(refused("journal changed during startup"));
    }

    // 10. Durable receipt, read back.
    let receipt_path = ownership_latch::sibling(&journal, ".startup-receipt")?;
    let receipt = Receipt {
        version: RECEIPT_VERSION,
        database: &actual,
        inventory_record_sha256: &record_digest,
        journal_sha256: hex::encode(Sha256::digest(&first)),
        authority: &authority,
        witness_profile: &entry.witness,
        witness_versions: &versions,
        nonce: hex::encode(nonce),
        admitted_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| refused("clock before epoch"))?
            .as_secs(),
    };
    let mut bytes = serde_json::to_vec(&receipt).map_err(|e| refused(format!("receipt: {e}")))?;
    bytes.push(b'\n');
    if bytes.len() > MAX_RECEIPT_BYTES {
        return Err(refused("receipt too large"));
    }
    write_receipt(&receipt_path, &bytes, &nonce)?;

    Ok(Admission {
        canonical,
        database: actual,
        journal,
        authority,
        binding,
        _latch: latch,
    })
}

/// Atomic, fsynced replace in the journal's protected directory, then an
/// exact read-back. Any failure is no admission.
pub(super) fn write_receipt(path: &Path, bytes: &[u8], nonce: &[u8; 32]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| refused("receipt has no directory"))?;
    let temporary = ownership_latch::sibling(path, &format!(".tmp-{}", hex::encode(&nonce[..8])))?;
    let result = (|| -> std::io::Result<()> {
        let mut file = ownership_latch::no_follow(OpenOptions::new().write(true).create_new(true))
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        File::open(dir)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| refused(format!("receipt durability: {e}")))?;
    let mut back = Vec::new();
    File::open(path)
        .and_then(|f| f.take(MAX_RECEIPT_BYTES as u64 + 1).read_to_end(&mut back))
        .map_err(|e| refused(format!("receipt read-back: {e}")))?;
    if back != bytes {
        return Err(refused("receipt read-back differs"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "ownership_admission_tests.rs"]
mod tests;
