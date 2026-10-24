# Shared applier: the library API

**Status:** proposed (2026-10-24). This is the API surface of the ONE
bootstrap-and-tail applier that `local-sync-mode.md` §3 requires: HA
followers, scoped followers, the in-page browser store and command-line
clients all configure the same component. The design rules (bootstrap, cut,
coverage, recovery) live in `local-sync-mode.md` §5 and §6; this document
fixes the types a consumer codes against, so a consumer can build to it
before the implementation lands.

## 1. Principles

- **No I/O inside.** The applier never opens a socket or reads a file.
  The caller fetches the bootstrap artifact and feeds frames from whatever
  transport it has (SSE, the WebSocket of `event-push.md`, a relay, a test
  fixture). The same code therefore compiles for the native server and for
  `wasm32`.
- **One cursor.** Progress is `(store_id, log_epoch, source_tx)` plus the
  last verified coverage and frame sequence, persisted in the same local
  transaction as the facts it covers.
- **Refuse rather than splice.** An epoch change, a scope mismatch, a
  sequence gap or an incomplete chunk never produces a partially applied
  state. The caller is told exactly what to do next.
- **Read-only replica.** The applied store refuses local mutation (follower
  profile). The client overlay (`local-sync-mode.md` §7, aegis-s24.4) sits on
  top and is not part of this API.

## 2. Types

```rust
/// Identity of the source log a replica follows.
pub struct Source { pub store_id: String, pub log_epoch: String }

/// From the bootstrap artifact (the as-of share, s24.1).
pub struct ReplicaManifest {
    pub source: Source,
    pub watermark_tx: i64,          // last source tx included in the snapshot
    pub scope_digest: String,       // sorted graph IRIs + declared context graphs
    pub policy_digest: String,
    pub shapes_hash: String,
    pub completeness: Completeness, // must cover valid time and lossless bytes
}

/// Persisted with every applied frame.
pub struct Progress {
    pub source: Source,
    pub source_tx: i64,             // last fully applied source transaction
    pub coverage: i64,              // last verified coverage watermark W
    pub session: Option<String>,    // stream session id, when frames carry one
    pub frame_seq: u64,
}

/// What a transport delivers, already parsed (see event-stream frames and
/// the transport-neutral FeedFrame{event, id, data}).
pub enum Frame {
    Tx { source: Source, source_tx: i64, frame_seq: u64, records: TxRecords },
    ChunkBegin { source: Source, source_tx: i64, parts: u32, digest: String, frame_seq: u64 },
    ChunkPart { source_tx: i64, index: u32, records: TxRecords, frame_seq: u64 },
    ChunkCommit { source_tx: i64, digest: String, frame_seq: u64 },
    Coverage { source: Source, watermark: i64, frame_seq: u64, signature: Option<Signature> },
    Control(Control),               // revocation, epoch end, protocol error
}

pub enum Outcome {
    Applied { source_tx: i64 },     // facts committed with the progress record
    Buffered { source_tx: i64 },    // chunk part held; nothing visible yet
    CoverageAdvanced { watermark: i64 },
    Duplicate { source_tx: i64 },   // at or below the cursor; ignored
    Resume { from: i64 },           // sequence gap: reconnect with since = from
    Rebootstrap { reason: String }, // epoch/scope/source changed: new share
    Revoked { reason: String },     // stop serving the affected scope
}

/// What a reader may claim about freshness.
pub struct Freshness {
    pub source_tx: i64,
    pub coverage: i64,
    pub coverage_age_ms: Option<u64>, // since the last verified coverage
    pub bounded: bool,                // true ONLY after a challenge (§5 of local-sync)
}
```

## 3. Operations

```rust
impl Applier {
    /// Load an as-of share into an EMPTY replica, or refuse. The share and
    /// its manifest are verified (completeness, scope, shapes) first.
    pub fn bootstrap(store: &mut Store, manifest: &ReplicaManifest, share: ShareReader) -> Result<Progress>;

    /// Re-open an existing replica: its persisted progress.
    pub fn open(store: &Store) -> Result<Progress>;

    /// Apply one frame. Exactly one local transaction per Applied outcome.
    pub fn apply(store: &mut Store, frame: Frame) -> Result<Outcome>;

    /// What may be said about this replica's freshness right now.
    pub fn freshness(store: &Store, now_ms: u64) -> Freshness;
}
```

Rules, in the order `apply` checks them:

1. `source` differs from the progress record → `Rebootstrap`.
2. A `Control` frame → acted on (`Revoked` or `Rebootstrap`), never filtered.
3. `frame_seq` not exactly `last + 1` within a session → `Resume { from:
   source_tx }`; nothing applied.
4. `source_tx <= progress.source_tx` → `Duplicate`.
5. Chunk parts buffer until `ChunkCommit`, whose digest must match the begin
   frame; a mismatch or a missing part → `Resume`, and nothing is visible.
6. A `Tx` (or a committed chunk) applies its records and the new progress in
   one local transaction → `Applied`.
7. `Coverage` updates only the progress record → `CoverageAdvanced`.

## 4. Phases against what ships today

- **Now (fixtures and the per-graph feed).** Until the as-of share (s24.1)
  and the scoped replication stream exist, a consumer may drive the applier
  from the shipped per-graph `/changes` pages: one graph per scope, `source`
  from the page's `store_id` and `log_epoch`, no coverage frames. Freshness
  is then `(source_tx, heartbeat age)` with `bounded = false`. A browser
  store built this way keeps the same seam: only the frame source changes
  later.
- **s24.1:** `bootstrap` from an as-of share, including frozen packs
  (aegis-s24.1.1).
- **s24.2:** the scoped stream with coverage and stream sessions; signed
  coverage through the shared signing crate (S0); the freshness challenge
  that makes `bounded = true` possible.

## 5. Acceptance (from local-sync-mode.md §12, P2 row)

Duplicate, reordered and dropped frames; an epoch change; a chunk cut
mid-part; revocation mid-chunk; restart and re-open; a quiet source passing
by coverage; a disconnected replica reporting UNKNOWN freshness. In every
case the replica equals the source at its cursor, or it holds no partial
state and has said what to do next.
