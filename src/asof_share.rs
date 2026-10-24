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
    /// Largest part, in bytes. Tests lower it to exercise splitting.
    pub part_max_len: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            read_timeout: Duration::from_secs(300),
            wal_growth_max_bytes: 1024 * 1024 * 1024,
            spool_quota_bytes: 8 * 1024 * 1024 * 1024,
            part_max_len: crate::feed_parts::MAX_FRAME_BYTES.saturating_sub(64),
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
    /// SHA-256 over each scope IRI then `\n`, an empty line, then each
    /// context IRI then `\n`, both lists sorted.
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
    /// The spool would pass [`Limits::spool_quota_bytes`].
    SpoolQuota,
}

impl AbortReason {
    /// Wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TimeCeiling => "time_ceiling",
            Self::WalCeiling => "wal_ceiling",
            Self::SpoolQuota => "spool_quota",
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

/// The scope digest of [`Manifest::scope_digest`].
#[must_use]
pub fn scope_digest(scope: &[String], context: &[String]) -> String {
    let mut text = String::new();
    for iri in scope {
        text.push_str(iri);
        text.push('\n');
    }
    text.push('\n');
    for iri in context {
        text.push_str(iri);
        text.push('\n');
    }
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

/// [`cut`] with a hook called after the head is read and at every ceiling
/// check (with the records written so far), so tests can commit writes
/// while the read transaction is open.
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
    // Refused before the transaction opens: a full spool directory must not
    // pin the WAL at all.
    if spool_usage(spool_dir)? >= limits.spool_quota_bytes {
        return Err(CutError::Aborted(AbortReason::SpoolQuota));
    }

    // Main's rows only. Resolution stays composed, so an attached id a main
    // row names still resolves; graphs served from an attachment are refused
    // below.
    let _main = store.scope_composition(false);
    let started = Instant::now();
    let wal = wal_path(store)?;
    let wal_start = wal_len(wal.as_deref());
    let read = store.conn.unchecked_transaction()?;

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
        let quota = limits
            .spool_quota_bytes
            .saturating_sub(spool_usage(spool_dir)?);
        let mut spooled: u64 = 0;
        let mut write_part = |part: String| -> std::result::Result<(), CutError> {
            spooled += part.len() as u64 + 1;
            if spooled > quota {
                return Err(CutError::Aborted(AbortReason::SpoolQuota));
            }
            file.write_all(part.as_bytes())?;
            file.write_all(b"\n")?;
            Ok(())
        };
        let mut builder = PartBuilder::with_max_len(watermark_tx, limits.part_max_len);
        let mut graphs = BTreeMap::new();
        let mut total = 0usize;
        for (iri, graph) in &selected {
            let mut hasher = Sha256::new();
            let mut records = 0usize;
            // idx_geav (g, e, a, v) gives this order without a sort, so the
            // read holds no temporary B-tree, and the same source at the same
            // T always sends the same records in the same order.
            let mut stmt = store.conn.prepare(
                "SELECT e, a, v, tx, valid_from, \
                        CASE WHEN retracted_tx IS NULL THEN valid_to END \
                 FROM facts INDEXED BY idx_geav \
                 WHERE g = ?1 AND op = 1 AND tx <= ?2 \
                   AND (retracted_tx IS NULL OR retracted_tx > ?2) \
                 ORDER BY e, a, v",
            )?;
            let mut rows = stmt.query(params![graph, watermark_tx])?;
            while let Some(row) = rows.next()? {
                let (e, a, v, tx, valid_from, valid_to): (
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
                );
                // The /changes assert record, minus the transaction metadata a
                // state cannot have: a later retract closes the row in place,
                // so a row retracted after T reports `valid_to: null` exactly
                // as the feed's assert record does.
                let record = json!({
                    "op": "assert",
                    "tx": tx,
                    "graph": iri,
                    "entity": store.resolve(e)?,
                    "attribute": store.resolve(a)?,
                    "value": store.value_json(&Value::from_bytes(&v)?)?,
                    "valid_from": valid_from,
                    "valid_to": valid_to,
                });
                hasher.update(record.to_string().as_bytes());
                hasher.update(b"\n");
                records += 1;
                total += 1;
                match builder.push(record) {
                    Ok(Some(part)) => write_part(part)?,
                    Ok(None) => {}
                    Err(RecordTooLarge) => {
                        return Err(invalid(format!(
                            "one record in {iri} exceeds the transport frame budget"
                        )));
                    }
                }
                if total.is_multiple_of(CHECK_EVERY) {
                    check_ceilings(started, limits, wal.as_deref(), wal_start)?;
                    during(total)?;
                }
            }
            graphs.insert(
                iri.clone(),
                GraphCompleteness {
                    records,
                    digest: format!("sha256:{}", hex::encode(hasher.finalize())),
                    frozen: None,
                },
            );
        }
        check_ceilings(started, limits, wal.as_deref(), wal_start)?;
        let (last, summary) = builder.finish();
        if let Some(part) = last {
            write_part(part)?;
        }
        file.flush()?;
        Ok((graphs, summary))
    })();
    // Closing the read transaction ends the pinned window, success or not.
    drop(read);
    let read_phase = started.elapsed();
    let (graphs, parts) = match written {
        Ok(done) => done,
        Err(error) => {
            let _ = std::fs::remove_file(&spool);
            return Err(error);
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
    })
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

fn spool_usage(dir: &Path) -> std::io::Result<u64> {
    let mut total = 0;
    for entry in std::fs::read_dir(dir)? {
        let meta = entry?.metadata()?;
        if meta.is_file() {
            total += meta.len();
        }
    }
    Ok(total)
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
