//! As-of scoped share: the replica bootstrap (docs/design/as-of-share.md).
//!
//! A follower starts from a cut of its source at transaction `T`, then tails
//! `/changes` from `T`. The cut is taken the way an acquisition is (one read
//! transaction, one validated `T`, complete or an error), but its payload is
//! change records, so bootstrap and tail use one record-to-datum conversion.
//!
//! The read transaction is open only while records are written to an on-disk
//! spool. Frames are sent from the spool afterwards, so a slow follower holds
//! a file, never the source's snapshot. Two ceilings bound the read phase:
//! wall time and WAL growth. Either one aborts the cut and deletes the spool;
//! no part of an aborted cut is ever sent.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::feed_parts::{PartBuilder, PartsSummary, RecordTooLarge};
use crate::types::Value;
use crate::{Error, Result, Store};

/// Contract version of the manifest.
pub const SCHEMA: &str = "quipu-asof-share/v1";

/// The single read rule in force until per-graph read authorization exists.
pub const POLICY: &str = "quipu-asof-policy/write-class-principal/v1";

/// The default graph's name in a request, a record and the manifest.
pub const ROOT: &str = "ROOT";

/// What the test hook is passed once the read transaction has closed.
pub(crate) const AFTER_READ: usize = usize::MAX;

/// Records between two ceiling checks during the read phase.
const CHECK_EVERY: usize = 10_000;

/// Exact source selection; no dataset expansion and no implicit ROOT.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Registered committed graphs the cut covers.
    pub graphs: Vec<String>,
    /// Further graphs the follower needs to read the scope (an ontology, for
    /// instance). Their records are carried too.
    #[serde(default)]
    pub context_graphs: Vec<String>,
    /// Transaction to cut at; absent means the head read inside the cut.
    #[serde(default, deserialize_with = "strict_tx")]
    pub tx: Option<i64>,
    /// Source lineage the caller expects; a mismatch is a refusal.
    pub expected_store_id: String,
}

fn strict_tx<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<i64>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    value
        .as_i64()
        .filter(|tx| *tx > 0)
        .map(Some)
        .ok_or_else(|| serde::de::Error::custom("tx must be a positive signed 64-bit JSON integer"))
}

/// Read-phase ceilings and the spool quota.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Wall time the read phase may take before the cut aborts.
    pub read_timeout: Duration,
    /// WAL growth since the cut began that aborts it.
    pub wal_growth_max_bytes: u64,
    /// Disk the spool directory may hold, this cut included.
    pub spool_quota_bytes: u64,
    /// Free disk a cut always leaves on the spool's filesystem.
    pub disk_floor_bytes: u64,
    /// Largest part, in bytes. Tests lower it to exercise splitting.
    pub part_max_len: usize,
    /// Canonical record lines held in memory, across every graph's digest,
    /// before the largest run is sorted and spilled to disk.
    pub digest_run_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            read_timeout: Duration::from_secs(300),
            wal_growth_max_bytes: 1024 * 1024 * 1024,
            spool_quota_bytes: 8 * 1024 * 1024 * 1024,
            disk_floor_bytes: 1024 * 1024 * 1024,
            part_max_len: crate::feed_parts::MAX_FRAME_BYTES.saturating_sub(64),
            digest_run_bytes: 32 * 1024 * 1024,
        }
    }
}

/// The source log a follower ties the cut to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Persistent source lineage.
    pub store_id: String,
    /// Current log epoch; a change means re-bootstrap, never splice.
    pub log_epoch: String,
}

/// What the cut holds for one graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphCompleteness {
    /// Live facts at `T`, one record each.
    pub records: usize,
    /// SHA-256 over the graph's record lines, each followed by `\n`, in the
    /// order they were sent.
    pub digest: String,
    /// The frozen pack the records were read from; always null until frozen
    /// graphs can be cut (aegis-s24.1.1).
    pub frozen: Option<serde_json::Value>,
}

/// What the cut can and cannot carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completeness {
    /// Records carry `valid_from` and `valid_to`.
    pub valid_time: bool,
    /// How byte values are carried.
    pub bytes: String,
    /// Every graph in the cut.
    pub graphs: BTreeMap<String, GraphCompleteness>,
}

/// Build that produced the cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Producer {
    /// Compiled version.
    pub version: String,
    /// Compiled source commit.
    pub git_sha: String,
    /// Whether the build had modified source.
    pub git_dirty: bool,
}

/// The cut's manifest. Its hash names the spool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// [`SCHEMA`].
    pub schema: String,
    /// Source identity read inside the cut.
    pub source: Source,
    /// Last source transaction included; the follower tails from it.
    pub watermark_tx: i64,
    /// Committed head when the cut was taken.
    pub source_head_tx: i64,
    /// Requested scope, sorted.
    pub scope: Vec<String>,
    /// Declared context graphs, sorted.
    pub context_graphs: Vec<String>,
    /// [`scope_digest`] of the scope and context lists.
    pub scope_digest: String,
    /// SHA-256 of the read rule the cut was served under ([`POLICY`]).
    pub policy_digest: String,
    /// SHA-256 of the shape sets in force at `T`, in the share encoding.
    pub shapes_hash: String,
    /// Per-graph record counts and digests.
    pub completeness: Completeness,
    /// Producing build.
    pub producer: Producer,
}

/// A finished cut: the manifest and the spool its parts were written to.
#[derive(Debug)]
pub struct Cut {
    /// The manifest.
    pub manifest: Manifest,
    /// SHA-256 of the manifest's compact JSON with sorted keys.
    pub manifest_sha256: String,
    /// The spool: one part's data per line, in order.
    pub spool: PathBuf,
    /// Part totals for the `begin` frame and the `commit` barrier.
    pub parts: PartsSummary,
    /// How long the read transaction was open.
    pub read_phase: Duration,
    /// Where the read phase's time went.
    pub profile: CutProfile,
}

/// Wall time per phase of the read phase, summed over every record.
#[derive(Debug, Clone, Default)]
pub struct CutProfile {
    /// SQLite stepping the scan: read, filter and row decode inside SQLite.
    pub sql_step: Duration,
    /// Copying the row out and decoding the value bytes.
    pub decode: Duration,
    /// The transaction's timestamp, actor and source.
    pub tx_meta: Duration,
    /// Entity and attribute IRIs.
    pub terms: Duration,
    /// The value's JSON form, including ref resolution.
    pub value_json: Duration,
    /// Building the record and serializing its canonical line.
    pub serialize: Duration,
    /// Holding the line for the graph digest, and spilling sorted runs.
    pub digest: Duration,
    /// Building parts and writing them to the spool.
    pub frame: Duration,
    /// Ceiling checks and the test hook.
    pub other: Duration,
    /// After the scan: merging digest runs and the last part.
    pub finish: Duration,
}

/// Why a cut was not produced.
#[derive(Debug)]
pub enum CutError {
    /// The request or the source state refuses the cut.
    Refused(Error),
    /// A ceiling stopped the read phase; nothing was sent and the spool is
    /// gone. The follower retries later.
    Aborted(AbortReason),
}

/// Which ceiling aborted a cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbortReason {
    /// The read phase ran past [`Limits::read_timeout`].
    TimeCeiling,
    /// The WAL grew past [`Limits::wal_growth_max_bytes`].
    WalCeiling,
    /// The spool would pass its allowance: the quota, or the free disk
    /// above [`Limits::disk_floor_bytes`].
    SpoolQuota,
    /// A spool write failed for lack of disk.
    DiskFull,
}

impl AbortReason {
    /// Wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TimeCeiling => "time_ceiling",
            Self::WalCeiling => "wal_ceiling",
            Self::SpoolQuota => "spool_quota",
            Self::DiskFull => "disk_full",
        }
    }
}

impl From<Error> for CutError {
    fn from(error: Error) -> Self {
        Self::Refused(error)
    }
}

impl From<rusqlite::Error> for CutError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Refused(error.into())
    }
}

impl From<std::io::Error> for CutError {
    fn from(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::StorageFull {
            return Self::Aborted(AbortReason::DiskFull);
        }
        Self::Refused(Error::Store(format!("as-of spool: {error}")))
    }
}

fn invalid(message: impl Into<String>) -> CutError {
    CutError::Refused(Error::InvalidValue(message.into()))
}

/// Compact UTF-8 JSON with sorted keys, the share-manifest encoding.
///
/// # Errors
///
/// Serialization failure.
pub fn manifest_bytes(manifest: &Manifest) -> Result<Vec<u8>> {
    let value = serde_json::to_value(manifest).map_err(|e| Error::InvalidValue(e.to_string()))?;
    serde_json::to_vec(&value).map_err(|e| Error::InvalidValue(e.to_string()))
}

/// The scope digest of [`Manifest::scope_digest`]: SHA-256 over the compact
/// JSON `{"context_graphs": [...], "scope": [...]}` with both lists sorted.
/// Each list is tagged, so moving an IRI from one list to the other always
/// changes the digest.
#[must_use]
pub fn scope_digest(scope: &[String], context: &[String]) -> String {
    let mut scope = scope.to_vec();
    let mut context = context.to_vec();
    scope.sort();
    context.sort();
    let text = json!({"scope": scope, "context_graphs": context}).to_string();
    crate::share::sha256(text.as_bytes())
}

/// Take a cut into a new spool under `spool_dir`.
///
/// # Errors
///
/// [`CutError::Refused`] for an invalid request or source state,
/// [`CutError::Aborted`] when a ceiling stops the read phase.
pub fn cut(
    store: &Store,
    request: &Request,
    spool_dir: &Path,
    limits: &Limits,
) -> std::result::Result<Cut, CutError> {
    cut_with_control(store, request, spool_dir, limits, |_| Ok(()))
}

/// [`cut`] with a hook called after the head is read, at every ceiling check
/// (with the records written so far), and with [`AFTER_READ`] once the read
/// transaction has closed, so tests can act while it is open and after.
pub(crate) fn cut_with_control(
    store: &Store,
    request: &Request,
    spool_dir: &Path,
    limits: &Limits,
    mut during: impl FnMut(usize) -> Result<()>,
) -> std::result::Result<Cut, CutError> {
    let scope = sorted_unique(&request.graphs, "graphs")?;
    let context = sorted_unique(&request.context_graphs, "context_graphs")?;
    if scope.is_empty() || scope.len() + context.len() > 64 {
        return Err(invalid(
            "as-of share requires 1..=64 explicit named graphs, context included",
        ));
    }
    if scope.iter().any(|iri| context.contains(iri)) {
        return Err(invalid("a graph is both in scope and in context"));
    }
    std::fs::create_dir_all(spool_dir)?;
    // The cut's disk allowance, fixed before the transaction opens: what the
    // quota leaves after every spool, run and cache already in the directory,
    // and never more than the free disk above the floor. One read phase runs
    // at a time, so no other cut is drawing on it. Parts and sorted runs are
    // charged against it BEFORE they are written. With no allowance the cut is
    // refused here and never pins the WAL.
    let allowance = limits
        .spool_quota_bytes
        .saturating_sub(spool_usage(spool_dir)?)
        .min(free_bytes(spool_dir).map_or(u64::MAX, |free| {
            free.saturating_sub(limits.disk_floor_bytes)
        }));
    if allowance == 0 {
        return Err(CutError::Aborted(AbortReason::SpoolQuota));
    }

    // Main's rows only. Resolution stays composed, so an attached id a main
    // row names still resolves; graphs served from an attachment are refused
    // below.
    let _main = store.scope_composition(false);
    let started = Instant::now();
    let wal = wal_path(store)?;
    let wal_start = wal_len(wal.as_deref());
    // The record-count checks below cannot interrupt one slow SQLite step, so
    // the same ceilings also run inside SQLite, every PROGRESS_STRIDE VM steps.
    // An interrupted step fails, and `interrupted` says which ceiling did it.
    let interrupted = Arc::new(AtomicU8::new(0));
    let _guard = ReadPhaseGuard::install(
        store,
        started,
        limits,
        wal.clone(),
        wal_start,
        Arc::clone(&interrupted),
    )?;
    let mut read = Some(store.conn.unchecked_transaction()?);
    let mut read_phase = None;

    let store_id = store.store_id()?;
    if store_id != request.expected_store_id {
        return Err(invalid("as-of share source store identity mismatch"));
    }
    let log_epoch = store.log_epoch()?;
    let head = store.latest_tx_id()?;
    during(0)?;
    let mut pin = json!({});
    if let Some(tx) = request.tx {
        pin["tx"] = json!(tx);
    }
    let watermark_tx = crate::read_pin::requested_tx(store, &pin)?.unwrap_or(head);
    if watermark_tx == 0 {
        return Err(invalid("as-of share source has no committed transaction"));
    }

    let mut selected = Vec::new();
    for iri in scope.iter().chain(&context) {
        // ROOT is never implied; a caller names it explicitly to cut it.
        if iri == ROOT {
            store.check_label_floor(&[0])?;
            selected.push((iri.clone(), 0));
            continue;
        }
        oxrdf::NamedNode::new(iri.clone()).map_err(|e| invalid(format!("graph IRI: {e}")))?;
        let Some(graph) = store.registered_graph_id(iri)? else {
            return Err(invalid(format!("unregistered graph: {iri}")));
        };
        if store.is_dataset(iri)? {
            return Err(invalid(format!("dataset IRI, not a graph: {iri}")));
        }
        if store.graph_class(graph)?.as_deref() != Some("committed") {
            return Err(invalid(format!(
                "as-of share requires a committed graph, not an overlay or fork: {iri}"
            )));
        }
        if store.attached_graphs().contains(&graph) {
            return Err(invalid(format!(
                "graph is served from an attached or frozen pack, which this \
                 producer cannot cut yet (aegis-s24.1.1): {iri}"
            )));
        }
        store.check_label_floor(&[graph])?;
        selected.push((iri.clone(), graph));
    }
    let shapes_hash = shapes_hash(store, watermark_tx)?;

    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let spool = spool_dir.join(format!(
        "cut-{}-{}.parts.tmp",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = std::io::BufWriter::new(std::fs::File::create(&spool)?);
        let quota = allowance;
        // Spool parts and spilled digest runs both count against the quota,
        // as they are written.
        let spooled = std::cell::Cell::new(0u64);
        let charge = |bytes: u64| -> std::result::Result<(), CutError> {
            spooled.set(spooled.get() + bytes);
            if spooled.get() > quota {
                return Err(CutError::Aborted(AbortReason::SpoolQuota));
            }
            Ok(())
        };
        let mut write_part = |part: String| -> std::result::Result<(), CutError> {
            charge(part.len() as u64 + 1)?;
            file.write_all(part.as_bytes())?;
            file.write_all(b"\n")?;
            Ok(())
        };
        let mut builder = PartBuilder::with_max_len(watermark_tx, limits.part_max_len);
        // One primary-key read per record: the /changes record names its
        // transaction's time, actor and source.
        let mut tx_meta = store
            .conn
            .prepare("SELECT timestamp, actor, source FROM transactions WHERE id = ?1")?;
        let mut graphs = BTreeMap::new();
        let mut total = 0usize;
        // One pass over facts in rowid order for every selected graph. Rowid
        // order is the order rows were written, so the read is sequential
        // rather than one table lookup per index entry, and consecutive rows
        // share a transaction, so its metadata is read once per transaction.
        // The per-graph digest sorts its lines, so emission order does not
        // reach it; the same source at the same T sends the same records in
        // the same order.
        let ids: Vec<String> = selected.iter().map(|(_, g)| g.to_string()).collect();
        let mut stmt = store.conn.prepare(&format!(
            "SELECT g, e, a, v, tx, valid_from, \
                    CASE WHEN retracted_tx IS NULL THEN valid_to END \
             FROM facts NOT INDEXED \
             WHERE g IN ({}) AND op = 1 AND tx <= ?1 \
               AND (retracted_tx IS NULL OR retracted_tx > ?1) \
             ORDER BY rowid",
            ids.join(",")
        ))?;
        let mut digests: BTreeMap<i64, (String, GraphDigest, usize)> = BTreeMap::new();
        for (iri, g) in &selected {
            digests.insert(*g, (iri.clone(), GraphDigest::new(spool_dir)?, 0));
        }
        let mut in_memory = 0usize;
        let mut profile = CutProfile::default();
        let mut meta: Option<(i64, TxMeta)> = None;
        let mut rows = stmt.query(params![watermark_tx])?;
        let mut clock = Instant::now();
        // Charge the time since the previous mark to one phase.
        let mut lap = |slot: &mut Duration| {
            let now = Instant::now();
            *slot += now - clock;
            clock = now;
        };
        while let Some(row) = rows.next()? {
            lap(&mut profile.sql_step);
            let (g, e, a, v, tx, valid_from, valid_to): (
                i64,
                i64,
                i64,
                Vec<u8>,
                i64,
                String,
                Option<String>,
            ) = (
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            );
            let value = Value::from_bytes(&v)?;
            lap(&mut profile.decode);
            if meta.as_ref().is_none_or(|m| m.0 != tx) {
                let fields = tx_meta.query_row(params![tx], |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                })?;
                meta = Some((tx, fields));
            }
            let (_, (timestamp, actor, source)) = meta.as_ref().expect("set above");
            lap(&mut profile.tx_meta);
            let (entity, attribute) = (store.resolve(e)?, store.resolve(a)?);
            lap(&mut profile.terms);
            let value = store.value_json(&value)?;
            lap(&mut profile.value_json);
            let (iri, digest, records) = digests.get_mut(&g).expect("selected graph");
            // The /changes assert record. A later retract closes the row in
            // place, so a row retracted after T reports `valid_to: null`
            // exactly as the feed's assert record does. Serialized once: the
            // same line feeds the graph digest and the part.
            let line = serde_json::to_string(&AssertRecord {
                actor,
                attribute: &attribute,
                entity: &entity,
                graph: iri,
                op: "assert",
                source,
                timestamp,
                tx,
                valid_from: &valid_from,
                valid_to: &valid_to,
                value: &value,
            })
            .map_err(|e| invalid(e.to_string()))?;
            lap(&mut profile.serialize);
            in_memory += line.len();
            digest.push(line.clone());
            *records += 1;
            total += 1;
            lap(&mut profile.digest);
            match builder.push_line(line) {
                Ok(Some(part)) => write_part(part)?,
                Ok(None) => {}
                Err(RecordTooLarge) => {
                    return Err(invalid(format!(
                        "one record in {iri} exceeds the transport frame budget"
                    )));
                }
            }
            lap(&mut profile.frame);
            // Every graph's run shares one memory budget: past it, the
            // largest run is spilled to disk.
            if in_memory >= limits.digest_run_bytes {
                let (_, largest, _) = digests
                    .values_mut()
                    .max_by_key(|(_, d, _)| d.pending())
                    .expect("at least one graph");
                in_memory -= largest.pending();
                charge(largest.spill()?)?;
                // A spill sorts and writes at most one run while the snapshot
                // is held; the ceilings are checked again after it.
                check_ceilings(started, limits, wal.as_deref(), wal_start)?;
            }
            lap(&mut profile.digest);
            if total.is_multiple_of(CHECK_EVERY) {
                check_ceilings(started, limits, wal.as_deref(), wal_start)?;
                during(total)?;
            }
            lap(&mut profile.other);
        }
        drop(rows);
        // The last word before a manifest exists: a cut of any size, even an
        // empty one, ends its read inside the ceilings or is aborted.
        check_ceilings(started, limits, wal.as_deref(), wal_start)?;
        // The scan is the only part that needs the snapshot. Close the read
        // transaction now, so merging digest runs and writing the last part
        // never pin the WAL, however long they take.
        drop(read.take());
        read_phase = Some(started.elapsed());
        during(AFTER_READ)?;
        let finishing = Instant::now();
        for (_, (iri, digest, records)) in digests {
            graphs.insert(
                iri,
                GraphCompleteness {
                    records,
                    digest: format!("sha256:{}", digest.finish()?),
                    frozen: None,
                },
            );
        }
        let (last, summary) = builder.finish();
        if let Some(part) = last {
            write_part(part)?;
        }
        file.flush()?;
        profile.finish = finishing.elapsed();
        Ok((graphs, summary, profile))
    })();
    // On an error path the read transaction may still be open: closing it
    // ends the pinned window, success or not.
    drop(read);
    drop(_guard);
    let read_phase = read_phase.unwrap_or_else(|| started.elapsed());
    let (graphs, parts, profile) = match written {
        Ok(done) => done,
        Err(error) => {
            let _ = std::fs::remove_file(&spool);
            return Err(match interrupted.load(Ordering::Acquire) {
                1 => CutError::Aborted(AbortReason::TimeCeiling),
                2 => CutError::Aborted(AbortReason::WalCeiling),
                _ => error,
            });
        }
    };

    let manifest = Manifest {
        schema: SCHEMA.into(),
        source: Source {
            store_id,
            log_epoch,
        },
        watermark_tx,
        source_head_tx: head,
        scope_digest: scope_digest(&scope, &context),
        scope,
        context_graphs: context,
        policy_digest: crate::share::sha256(POLICY.as_bytes()),
        shapes_hash,
        completeness: Completeness {
            valid_time: true,
            bytes: "lossless".into(),
            graphs,
        },
        producer: Producer {
            version: env!("CARGO_PKG_VERSION").into(),
            git_sha: env!("QUIPU_GIT_SHA").into(),
            git_dirty: env!("QUIPU_GIT_DIRTY") == "true",
        },
    };
    let manifest_sha256 = crate::share::sha256(&manifest_bytes(&manifest)?);
    let named = spool_dir.join(format!(
        "{}.parts",
        manifest_sha256.trim_start_matches("sha256:")
    ));
    std::fs::rename(&spool, &named)?;
    Ok(Cut {
        manifest,
        manifest_sha256,
        spool: named,
        parts,
        read_phase,
        profile,
    })
}

/// One cut record in the `/changes` assert form. Fields are declared in
/// alphabetical order, so `serde_json` writes the same canonical line as a
/// `Value` with sorted keys, without building a map per record.
#[derive(Serialize)]
pub(crate) struct AssertRecord<'a> {
    pub(crate) actor: &'a Option<String>,
    pub(crate) attribute: &'a str,
    pub(crate) entity: &'a str,
    pub(crate) graph: &'a str,
    pub(crate) op: &'static str,
    pub(crate) source: &'a Option<String>,
    pub(crate) timestamp: &'a Option<String>,
    pub(crate) tx: i64,
    pub(crate) valid_from: &'a str,
    pub(crate) valid_to: &'a Option<String>,
    pub(crate) value: &'a serde_json::Value,
}

/// A transaction's `timestamp`, `actor` and `source`.
type TxMeta = (Option<String>, Option<String>, Option<String>);

/// VM steps between two ceiling checks inside SQLite.
const PROGRESS_STRIDE: i32 = 1000;

/// The read phase's ceilings, installed as the connection's progress handler
/// for as long as the cut reads, and removed on drop.
pub(crate) struct ReadPhaseGuard<'s> {
    store: &'s Store,
}

impl<'s> ReadPhaseGuard<'s> {
    pub(crate) fn install(
        store: &'s Store,
        started: Instant,
        limits: &Limits,
        wal: Option<PathBuf>,
        wal_start: u64,
        interrupted: Arc<AtomicU8>,
    ) -> Result<Self> {
        let (timeout, wal_max) = (limits.read_timeout, limits.wal_growth_max_bytes);
        // The WAL is a file stat: at most once a second, not every stride.
        let mut wal_checked = started;
        store.conn.progress_handler(
            PROGRESS_STRIDE,
            Some(move || {
                if started.elapsed() > timeout {
                    interrupted.store(1, Ordering::Release);
                    return true;
                }
                if wal_checked.elapsed() >= Duration::from_secs(1) {
                    wal_checked = Instant::now();
                    if wal_len(wal.as_deref()).saturating_sub(wal_start) > wal_max {
                        interrupted.store(2, Ordering::Release);
                        return true;
                    }
                }
                false
            }),
        )?;
        Ok(Self { store })
    }
}

impl Drop for ReadPhaseGuard<'_> {
    fn drop(&mut self) {
        let _ = self.store.conn.progress_handler(0, None::<fn() -> bool>);
    }
}

fn sorted_unique(iris: &[String], field: &str) -> std::result::Result<Vec<String>, CutError> {
    let set: BTreeSet<&String> = iris.iter().collect();
    if set.len() != iris.len() {
        return Err(invalid(format!("duplicate graph in {field}")));
    }
    Ok(set.into_iter().cloned().collect())
}

fn check_ceilings(
    started: Instant,
    limits: &Limits,
    wal: Option<&Path>,
    wal_start: u64,
) -> std::result::Result<(), CutError> {
    if started.elapsed() > limits.read_timeout {
        return Err(CutError::Aborted(AbortReason::TimeCeiling));
    }
    if wal_len(wal).saturating_sub(wal_start) > limits.wal_growth_max_bytes {
        return Err(CutError::Aborted(AbortReason::WalCeiling));
    }
    Ok(())
}

/// The main database's WAL file, or `None` for an in-memory store.
fn wal_path(store: &Store) -> Result<Option<PathBuf>> {
    let file: String = store.conn.query_row(
        "SELECT file FROM pragma_database_list WHERE name = 'main'",
        [],
        |r| r.get(0),
    )?;
    Ok((!file.is_empty()).then(|| PathBuf::from(format!("{file}-wal"))))
}

fn wal_len(wal: Option<&Path>) -> u64 {
    wal.and_then(|path| std::fs::metadata(path).ok())
        .map_or(0, |meta| meta.len())
}

/// Every byte under the spool directory: finished spools still being sent,
/// a partial spool and sorted digest runs.
fn spool_usage(dir: &Path) -> std::io::Result<u64> {
    let mut total = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_dir() {
            total += spool_usage(&entry.path())?;
        } else {
            total += meta.len();
        }
    }
    Ok(total)
}

/// Free bytes on the filesystem holding `dir`, for an unprivileged writer.
#[cfg(unix)]
fn free_bytes(dir: &Path) -> Option<u64> {
    let stat = rustix::fs::statvfs(dir).ok()?;
    Some(stat.f_bavail.saturating_mul(stat.f_frsize))
}

#[cfg(not(unix))]
fn free_bytes(_dir: &Path) -> Option<u64> {
    None
}

/// The applier's per-graph share digest (`applier::share_graph_digest`):
/// SHA-256 over the graph's canonical record lines, sorted, joined by `\n`.
///
/// The applier sorts in memory. A producer cutting millions of records cannot,
/// so lines are sorted in runs of at most `run_bytes`, spilled to a temporary
/// directory beside the spool, and merged into the hash. The bytes hashed are
/// the same.
struct GraphDigest {
    dir: tempfile::TempDir,
    run: Vec<String>,
    run_len: usize,
    runs: Vec<PathBuf>,
}

impl GraphDigest {
    fn new(spool_dir: &Path) -> std::io::Result<Self> {
        Ok(Self {
            dir: tempfile::Builder::new()
                .prefix("digest-runs-")
                .tempdir_in(spool_dir)?,
            run: Vec::new(),
            run_len: 0,
            runs: Vec::new(),
        })
    }

    /// Hold one line in the current run; the caller decides when to spill.
    fn push(&mut self, line: String) {
        self.run_len += line.len();
        self.run.push(line);
    }

    /// Bytes held in memory, not yet spilled.
    fn pending(&self) -> usize {
        self.run_len
    }

    fn spill(&mut self) -> std::io::Result<u64> {
        let bytes = (self.run_len + self.run.len()) as u64;
        self.run.sort_unstable();
        let path = self.dir.path().join(format!("run-{}", self.runs.len()));
        let mut file = std::io::BufWriter::new(std::fs::File::create(&path)?);
        for line in self.run.drain(..) {
            file.write_all(line.as_bytes())?;
            file.write_all(b"\n")?;
        }
        file.flush()?;
        self.runs.push(path);
        self.run_len = 0;
        Ok(bytes)
    }

    /// Lowercase hex of the digest. The run directory is removed on drop.
    fn finish(mut self) -> std::io::Result<String> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        use std::io::BufRead;
        let mut hasher = Sha256::new();
        let mut first = true;
        let mut emit = |line: &str| {
            if !first {
                hasher.update(b"\n");
            }
            first = false;
            hasher.update(line.as_bytes());
        };
        if self.runs.is_empty() {
            self.run.sort_unstable();
            for line in &self.run {
                emit(line);
            }
        } else {
            if !self.run.is_empty() {
                self.spill()?;
            }
            let mut readers = self
                .runs
                .iter()
                .map(|path| std::fs::File::open(path).map(|f| std::io::BufReader::new(f).lines()))
                .collect::<std::io::Result<Vec<_>>>()?;
            let mut heap = BinaryHeap::new();
            for (index, reader) in readers.iter_mut().enumerate() {
                if let Some(line) = reader.next().transpose()? {
                    heap.push(Reverse((line, index)));
                }
            }
            while let Some(Reverse((line, index))) = heap.pop() {
                emit(&line);
                if let Some(next) = readers[index].next().transpose()? {
                    heap.push(Reverse((next, index)));
                }
            }
        }
        Ok(hex::encode(hasher.finalize()))
    }
}

/// The shape sets in force at `tx`, in the share's encoding.
fn shapes_hash(store: &Store, tx: i64) -> Result<String> {
    let sets = store.list_shapes_as_of(&crate::store::AsOf {
        tx: Some(tx),
        valid_at: None,
    })?;
    let sets: BTreeMap<&str, &str> = sets
        .iter()
        .map(|(name, turtle, _)| (name.as_str(), turtle.as_str()))
        .collect();
    let mut out = String::new();
    for (name, turtle) in sets {
        out.push_str(&format!("# --- {name} ---\n{}\n", turtle.trim_end()));
    }
    Ok(crate::share::sha256(out.as_bytes()))
}
