//! Owner enrollment (aegis-4qv.2): the library path that turns an existing
//! native Quipu file into an enrolled S1 owner over a create-only witness.
//!
//! Enrollment is exactly what the private admission fixture does, made
//! durable and resumable: witness genesis (one conditional create, never an
//! overwrite), an Active journal, an owner-only inventory, the database
//! marker, and finally a real `Store::open_enrolled` against the same witness.
//!
//! The order makes every partial failure fail-closed and resumable:
//!   1. preflight, read-only: db identity, no marker, no inventory, journal and
//!      witness either absent or holding exactly the planned bytes;
//!   2. persist an owner-only INTENT before any remote write;
//!   3. witness genesis create (identical existing bytes resume, different
//!      bytes refuse as a lineage conflict; nothing is ever deleted);
//!   4. journal, then inventory, each published without replacing a name;
//!   5. the marker binding in one SQLite transaction, after re-checking the
//!      database device/inode;
//!   6. `open_enrolled` through real S1 admission; only then retire the intent.
//!
//! The planned authority is a pure function of the database identity and the
//! request, so a resumed run recomputes the identical plan and the intent is
//! only permission to accept the earlier steps' bytes. Startup already refuses
//! a marker without a valid inventory, journal and witness, so a partial
//! enrollment never serves.
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::Store;
use super::ownership_admission::{
    EnrolledStartup, MARKER_SQL, MarkerRead, WitnessSource, database, owner_only_regular,
    read_marker,
};
use super::ownership_inventory::{Binding, Database, Entry, Inventory};
use super::ownership_journal::{self, JournalState};
use crate::error::{Error, Result};
use crate::ownership_handoff_model::witness::objects::{Created, Objects};
use crate::ownership_handoff_model::witness::{
    LINEAGE_LIST_LIMIT, ObjectWitness, Reply, genesis_record,
};
use crate::ownership_handoff_model::{Authority, AuthorityUnit, Journal, Placement, WitnessHead};

const INTENT_VERSION: u32 = 1;

pub(super) fn refused(why: impl std::fmt::Display) -> Error {
    Error::PolicyDenied(format!("store handoff: enrollment refused: {why}"))
}

/// A genesis-capable witness: the create-only backend plus the profile name
/// the inventory will record for it.
pub(crate) struct EnrollWitness<'a, B: Objects> {
    pub(crate) profile: &'a str,
    pub(crate) objects: &'a B,
}

pub(crate) struct EnrollRequest<'a, B: Objects> {
    pub(crate) db_path: &'a Path,
    pub(crate) journal_path: &'a Path,
    pub(crate) unit: AuthorityUnit,
    /// Profile named by the operator's configuration; must equal the witness.
    pub(crate) profile: &'a str,
    /// Existing principal that owns this placement.
    pub(crate) principal: &'a str,
    /// Served writer policy the catalog pins.
    pub(crate) policy: &'a str,
    pub(crate) witness: EnrollWitness<'a, B>,
}

/// What enrollment established. Deterministic for one file and request, so a
/// resumed run returns the identical receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct EnrollReceipt {
    pub(crate) store_id: String,
    pub(crate) generation: u64,
    pub(crate) inventory_digest: String,
    pub(crate) witness_key: String,
    pub(crate) database: Database,
}

/// Dry-run result: the preflight passed and this is what would be written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct EnrollPlan {
    pub(crate) authority: Authority,
    pub(crate) inventory: PathBuf,
    pub(crate) journal: PathBuf,
    pub(crate) intent: PathBuf,
    /// An intent from an interrupted run exists and matches this plan.
    pub(crate) resume: bool,
    pub(crate) receipt: EnrollReceipt,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u32,
    authority: Authority,
    entry: Entry,
    genesis_sha256: String,
}

/// Everything step 1 established, carried to the writing steps.
struct Prepared {
    plan: EnrollPlan,
    canonical_db: PathBuf,
    entry: Entry,
    binding: Binding,
    inventory_bytes: Vec<u8>,
    intent_bytes: Vec<u8>,
    genesis_key: String,
    genesis_bytes: Vec<u8>,
    genesis_present: bool,
    journal_present: bool,
    inventory_present: bool,
    marker_present: bool,
}

/// Test-only fault seams: stop after a step, or act just before step 5.
#[derive(Default)]
pub(super) struct Hooks<'a> {
    pub(super) crash_after: Option<u8>,
    pub(super) before_marker: Option<&'a dyn Fn()>,
}

/// Step 1 only: read-only preflight and the planned authority.
pub(crate) fn plan_enrollment<B: Objects>(req: &EnrollRequest<'_, B>) -> Result<EnrollPlan> {
    Ok(prepare(req)?.plan)
}

/// Perform (or resume) enrollment and prove it through S1 admission.
pub(crate) fn enroll_owner<B: Objects>(req: &EnrollRequest<'_, B>) -> Result<EnrollReceipt> {
    run(req, &Hooks::default())
}

pub(super) fn run<B: Objects>(
    req: &EnrollRequest<'_, B>,
    hooks: &Hooks<'_>,
) -> Result<EnrollReceipt> {
    let crash = |step: u8| match hooks.crash_after {
        Some(n) if n == step => Err(refused(format!("injected crash after step {step}"))),
        _ => Ok(()),
    };
    // 1. Preflight.
    let p = prepare(req)?;
    // 2. Intent before any remote write.
    if !p.plan.resume {
        publish_new(&p.plan.intent, &p.intent_bytes, "intent")?;
    }
    crash(2)?;
    // 3. Witness genesis: conditional create, exact readback on doubt.
    if !p.genesis_present {
        match req.witness.objects.create(&p.genesis_key, &p.genesis_bytes) {
            Reply::Ok(Created::New) => {}
            Reply::Ok(Created::Exists) => match read_genesis(req, &p.genesis_key)? {
                Some(bytes) if bytes == p.genesis_bytes => {}
                _ => return Err(refused("witness genesis exists with different bytes")),
            },
            _ => {
                return Err(refused(
                    "witness genesis create is UNKNOWN; re-run to resume from the intent",
                ));
            }
        }
    }
    crash(3)?;
    // 4. Journal, then inventory; neither replaces an existing name.
    if !p.journal_present {
        ownership_journal::create(
            &p.plan.journal,
            &Journal::Active {
                authority: p.plan.authority.clone(),
            },
        )?;
    }
    if !p.inventory_present {
        publish_new(&p.plan.inventory, &p.inventory_bytes, "inventory")?;
    }
    crash(4)?;
    // 5. Marker, in one transaction, on the file preflight identified.
    if let Some(hook) = hooks.before_marker {
        hook();
    }
    if !p.marker_present {
        write_marker(&p)?;
    }
    crash(5)?;
    // 6. Prove it through the real startup path, then retire the intent.
    let source = VerifySource {
        profile: req.witness.profile,
        witness: ObjectWitness::attach(p.plan.authority.store_id.clone(), req.witness.objects),
    };
    let store = Store::open_enrolled(
        path_str(&p.canonical_db)?,
        &EnrolledStartup {
            inventory: &p.plan.inventory,
            witness: &source,
        },
    )?;
    drop(store);
    fs::remove_file(&p.plan.intent).map_err(|e| refused(format!("retire intent: {e}")))?;
    sync_dir(&p.plan.intent)?;
    Ok(p.plan.receipt)
}

fn prepare<B: Objects>(req: &EnrollRequest<'_, B>) -> Result<Prepared> {
    if req.profile != req.witness.profile {
        return Err(refused("witness profile differs from the configuration"));
    }
    // Database: a regular single-link file, never a symlink.
    let link = fs::symlink_metadata(req.db_path).map_err(|e| refused(format!("database: {e}")))?;
    if !link.file_type().is_file() {
        return Err(refused("database path is a symlink or not a regular file"));
    }
    let (canonical_db, db) = database(req.db_path)?;
    let (store_id, epoch, marker) = read_db(&canonical_db)?;

    let journal = canonical_sibling_path(req.journal_path)?;
    for suffix in ["", "-wal", "-shm", "-journal"] {
        if journal.as_os_str() == format!("{}{suffix}", db.path).as_str() {
            return Err(refused("journal is the database or one of its sidecars"));
        }
    }
    let inventory = journal.with_extension("inventory");
    let intent_path = journal.with_extension("enroll-intent");

    let authority = Authority {
        store_id,
        unit: req.unit.clone(),
        generation: 0,
        epoch,
        owner: Placement {
            principal: req.principal.into(),
            native: native_digest(&db),
        },
        policy: req.policy.into(),
        predecessor: None,
    };
    authority
        .validate()
        .map_err(|e| refused(format!("planned authority: {e:?}")))?;
    let entry = Entry {
        store_id: authority.store_id.clone(),
        unit: authority.unit.clone(),
        database: db.clone(),
        journal: path_str(&journal)?.into(),
        witness: req.profile.into(),
        enrolled_at_generation: 0,
    };
    let inventory_digest = entry.digest().map_err(refused)?;
    let binding = Binding {
        version: 1,
        store_id: authority.store_id.clone(),
        inventory_digest: inventory_digest.clone(),
    };
    let inventory_bytes = Inventory {
        version: 1,
        records: vec![entry.clone()],
    }
    .encode()
    .map_err(refused)?;
    let (genesis_key, genesis_bytes) =
        genesis_record(&authority).ok_or_else(|| refused("genesis record cannot be encoded"))?;
    let intent = Intent {
        version: INTENT_VERSION,
        authority: authority.clone(),
        entry: entry.clone(),
        genesis_sha256: hex::encode(Sha256::digest(&genesis_bytes)),
    };
    let mut intent_bytes = serde_json::to_vec(&intent).map_err(refused)?;
    intent_bytes.push(b'\n');

    // An intent is only ever this exact plan.
    let resume = match exists(&intent_path)? {
        false => false,
        true => {
            owner_only_regular(&intent_path, "intent", 0o077)?;
            if fs::read(&intent_path).map_err(refused)? != intent_bytes {
                return Err(refused("a different enrollment intent exists"));
            }
            true
        }
    };
    let marker_present = match marker {
        MarkerRead::Absent => false,
        MarkerRead::Known(found) if resume && found == binding => true,
        MarkerRead::Known(_) => return Err(refused("database already has an enrollment marker")),
        MarkerRead::Unknown => return Err(refused("enrollment marker is UNKNOWN")),
    };
    let inventory_present = match exists(&inventory)? {
        false => false,
        true if resume => {
            owner_only_regular(&inventory, "inventory", 0o077)?;
            if fs::read(&inventory).map_err(refused)? != inventory_bytes {
                return Err(refused("inventory differs from the enrollment intent"));
            }
            true
        }
        true => return Err(refused("an inventory already exists")),
    };
    let journal_present = match exists(&journal)? {
        false => false,
        true => {
            owner_only_regular(&journal, "journal", 0o077)?;
            match ownership_journal::read(&journal) {
                JournalState::Known(j)
                    if *j
                        == (Journal::Active {
                            authority: authority.clone(),
                        }) => {}
                _ => return Err(refused("journal exists and is not this enrollment's")),
            }
            true
        }
    };
    let genesis_present = witness_state(req, &genesis_key, &genesis_bytes)?;

    let receipt = EnrollReceipt {
        store_id: authority.store_id.clone(),
        generation: authority.generation,
        inventory_digest,
        witness_key: genesis_key.clone(),
        database: db,
    };
    Ok(Prepared {
        plan: EnrollPlan {
            authority,
            inventory,
            journal,
            intent: intent_path,
            resume,
            receipt,
        },
        canonical_db,
        entry,
        binding,
        inventory_bytes,
        intent_bytes,
        genesis_key,
        genesis_bytes,
        genesis_present,
        journal_present,
        inventory_present,
        marker_present,
    })
}

/// The lineage prefix is empty, or holds exactly the planned genesis.
fn witness_state<B: Objects>(req: &EnrollRequest<'_, B>, key: &str, bytes: &[u8]) -> Result<bool> {
    let Reply::Ok(keys) = req.witness.objects.list(LINEAGE_LIST_LIMIT) else {
        return Err(refused("witness is unreachable or its listing is UNKNOWN"));
    };
    match keys.as_slice() {
        [] => Ok(false),
        [only] if only == key => match read_genesis(req, key)? {
            Some(found) if found == bytes => Ok(true),
            Some(_) => Err(refused(
                "witness genesis exists with different bytes (lineage conflict)",
            )),
            None => Err(refused("witness listing and read disagree")),
        },
        _ => Err(refused("witness lineage prefix is not empty")),
    }
}

fn read_genesis<B: Objects>(req: &EnrollRequest<'_, B>, key: &str) -> Result<Option<Vec<u8>>> {
    match req.witness.objects.get(key) {
        Reply::Ok(found) => Ok(found.map(|o| o.bytes)),
        _ => Err(refused("witness genesis read is UNKNOWN")),
    }
}

fn read_db(path: &Path) -> Result<(String, String, MarkerRead)> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let one = |sql: &str| -> Result<String> {
        conn.query_row(sql, [], |r| r.get(0))
            .map_err(|_| refused("database is not an initialized Quipu store"))
    };
    let store_id = one("SELECT store_id FROM store_identity WHERE id = 1")?;
    let epoch = one("SELECT epoch FROM log_epoch WHERE id = 1")?;
    let marker = read_marker(&conn)?;
    Ok((store_id, epoch, marker))
}

fn write_marker(p: &Prepared) -> Result<()> {
    let (_, now) = database(&p.canonical_db)?;
    if now != p.entry.database {
        return Err(refused("database device/inode changed since preflight"));
    }
    let mut conn = Connection::open(&p.canonical_db)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if read_marker(&tx)? != MarkerRead::Absent {
        return Err(refused("an enrollment marker appeared since preflight"));
    }
    tx.execute_batch(MARKER_SQL)?;
    tx.execute(
        "INSERT INTO ownership_handoff_enrollment(id, binding) VALUES (1, ?1)",
        [serde_json::to_string(&p.binding).map_err(refused)?],
    )?;
    tx.commit()?;
    Ok(())
}

struct VerifySource<'a, B: Objects> {
    profile: &'a str,
    witness: ObjectWitness<&'a B>,
}

impl<B: Objects> WitnessSource for VerifySource<'_, B> {
    fn head_evidence(
        &self,
        profile: &str,
        store_id: &str,
        anchor: u64,
        _nonce: &[u8; 32],
    ) -> Reply<(WitnessHead, Vec<(String, String)>)> {
        if profile != self.profile {
            return Reply::Unknown;
        }
        self.witness.head_evidence(store_id, anchor)
    }
}

/// Digest of the native host/path binding the placement names.
pub(super) fn native_digest(db: &Database) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"quipu-enroll-native/v1\0");
    h.update(db.path.as_bytes());
    h.update(b"\0");
    h.update(db.device.to_be_bytes());
    h.update(db.inode.to_be_bytes());
    h.finalize().into()
}

fn canonical_sibling_path(path: &Path) -> Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| refused("journal path has no file name"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| refused("journal path has no directory"))?;
    let dir = parent
        .canonicalize()
        .map_err(|e| refused(format!("journal directory: {e}")))?;
    Ok(dir.join(name))
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(refused(format!("cannot establish absence: {e}"))),
    }
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| refused("path is not UTF-8"))
}

/// Owner-only, fsynced, published with a hard link so an existing name is
/// never replaced (tmp + link + unlink, then a directory fsync).
fn publish_new(path: &Path, bytes: &[u8], what: &str) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension(format!("{what}.tmp-{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| refused(format!("{what} staging: {e}")))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let published = written.and_then(|()| fs::hard_link(&tmp, path));
    let _ = fs::remove_file(&tmp);
    published.map_err(|e| refused(format!("{what} publish: {e}")))?;
    sync_dir(path)
}

fn sync_dir(path: &Path) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| refused(format!("directory fsync: {e}")))
}

#[cfg(test)]
#[path = "ownership_enroll_tests.rs"]
mod tests;
