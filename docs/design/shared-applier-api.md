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

/// The retained identity of one applied transaction, persisted in the same
/// local transaction as its facts (see "Transaction digest" below).
pub struct AppliedTx { pub source: Source, pub source_tx: i64, pub tx_digest: String }

/// A freshness challenge the caller is about to send (local-sync §5).
/// The caller supplies the nonce from its own CSPRNG and `t_send_ms` from
/// the SAME monotonic clock it later passes to `freshness`; the applier
/// does no I/O and reads no clock.
pub struct Challenge { pub session: String, pub nonce: [u8; 16], pub t_send_ms: u64 }

/// What a transport delivers, already parsed (see event-stream frames and
/// the transport-neutral FeedFrame{event, id, data}).
pub enum Frame {
    Tx { source: Source, source_tx: i64, frame_seq: u64, records: TxRecords },
    ChunkBegin { source: Source, source_tx: i64, parts: u32, digest: String, frame_seq: u64 },
    ChunkPart { source_tx: i64, index: u32, records: TxRecords, frame_seq: u64 },
    ChunkCommit { source_tx: i64, digest: String, frame_seq: u64 },
    Coverage {
        source: Source, watermark: i64, frame_seq: u64,
        signature: Option<Signature>,
        session: Option<String>,    // the stream session the frame was sent on
        nonce: Option<[u8; 16]>,    // echo of a challenge; None for a heartbeat
    },
    Control(Control),               // revocation, epoch end, protocol error
}

pub enum Outcome {
    Applied { source_tx: i64 },     // facts committed with the progress record
    Buffered { source_tx: i64 },    // chunk part held; nothing visible yet
    CoverageAdvanced { watermark: i64 },
    Duplicate { source_tx: i64 },   // at or below the cursor, digest EQUAL to the applied one
    ProofAccepted { t_send_ms: u64 }, // a valid answer to an outstanding challenge
    Resume { from: i64 },           // sequence gap: reconnect with since = from
    Rebootstrap { reason: String }, // epoch/scope/source changed: new share
    Revoked { reason: String },     // stop serving the affected scope
    Stopped { reason: StopReason }, // terminal: every later apply returns it too
}

pub enum StopReason {
    /// A redelivered transaction differs from the one already applied.
    DigestMismatch { source_tx: i64, applied: String, received: String },
    /// A redelivered transaction has no retained digest to compare against
    /// (below the retained floor, or covered only by the bootstrap snapshot).
    DigestUnverifiable { source_tx: i64 },
}

/// What a reader may claim about freshness.
pub struct Freshness {
    pub source_tx: i64,
    pub coverage: i64,
    pub coverage_age_ms: Option<u64>,   // since the last verified coverage; never a freshness bound
    /// `now_ms - t_send_ms` of the latest valid proof, while the cursor is
    /// at or past that proof's W. `None` = freshness UNKNOWN.
    pub staleness_bound_ms: Option<u64>,
}

impl Freshness {
    /// A `bounded=<max_ms>` read (local-sync §9) may be served.
    pub fn within(&self, max_ms: u64) -> bool { self.staleness_bound_ms.is_some_and(|b| b <= max_ms) }
}
```

## 3. Operations

```rust
impl Applier {
    /// Load an as-of share into an EMPTY replica, or refuse. The share and
    /// its manifest are verified (completeness, scope, shapes) first.
    pub fn bootstrap(store: &mut Store, manifest: &ReplicaManifest, share: ShareReader) -> Result<Applier>;

    /// Re-open an existing replica from its persisted progress. Proofs and
    /// outstanding challenges are process memory only: a new instance has none.
    pub fn open(store: &Store) -> Result<Applier>;

    /// The persisted cursor.
    pub fn progress(&self) -> &Progress;

    /// Apply one frame. Exactly one local transaction per Applied outcome.
    pub fn apply(&mut self, store: &mut Store, frame: Frame) -> Result<Outcome>;

    /// Record a challenge before the caller sends it. Refused if `session`
    /// is not the current stream session or `nonce` was issued before.
    pub fn challenge(&mut self, challenge: Challenge) -> Result<()>;

    /// The caller's monotonic clock was discontinuous (suspend detected):
    /// discard every proof and outstanding challenge.
    pub fn clock_discontinuity(&mut self);

    /// What may be said about this replica's freshness at `now_ms`, read
    /// from the same monotonic clock as every `t_send_ms`.
    pub fn freshness(&self, now_ms: u64) -> Freshness;
}
```

**Transaction digest.** `tx_digest` is the identity of a transaction's
CONTENT, independent of how it was framed (one `Tx` frame, a `/changes`
page, or a chunked begin/part/commit). The applier computes it itself from
the decoded records it would write, never from a digest the frame declares:
SHA-256 over the domain tag `quipu-applier-tx/v1`, then `store_id`,
`log_epoch` and `source_tx`, then each record's lossless canonical encoding
(graph, subject, predicate, object with its exact value bytes and datatype,
valid-from, valid-to, op), sorted bytewise, every field length-prefixed. The
chunk digests of §6 prove one delivery's integrity and are not compared
across deliveries: a redelivery may be split differently.

`AppliedTx` rows are kept for every transaction applied by the stream since
bootstrap, down to a retained floor the caller may raise (default: all). The
bootstrap snapshot has no per-transaction digests, so the floor is never
below `manifest.watermark_tx`.

**Freshness proof.** `challenge` records `(session, nonce, t_send_ms)` as
outstanding. A `Coverage` frame with a `nonce` is a proof only if its
signature verifies, its `session` is the current session, and its `nonce` is
outstanding; the challenge is then consumed, so a replayed or unsolicited
answer is never a proof. The proof holds `(t_send_ms, W)`; `freshness`
reports `now_ms - t_send_ms` of the latest proof while `progress.source_tx >=
W`, and `None` otherwise. A heartbeat (`nonce: None`) only advances coverage,
never freshness. A session change, `clock_discontinuity` or a new instance
discards all proofs.

Rules, in the order `apply` checks them:

1. `source` differs from the progress record → `Rebootstrap`.
2. A `Control` frame → acted on (`Revoked` or `Rebootstrap`), never filtered.
3. `frame_seq` not exactly `last + 1` within a session → `Resume { from:
   source_tx }`; nothing applied.
4. After a `Stopped`, every frame → the same `Stopped`; nothing applied.
5. Chunk parts buffer until `ChunkCommit`, whose digest must match the begin
   frame; a mismatch or a missing part → `Resume`, and nothing is visible.
   A redelivered chunk (`source_tx <= progress.source_tx`) is staged and
   verified the same way, then goes to rule 6; it is never applied.
6. `source_tx <= progress.source_tx` (a `Tx`, or a committed chunk): compute
   its `tx_digest` and compare it with the retained `AppliedTx`. Equal →
   `Duplicate`. Different → `Stopped { DigestMismatch }`. No retained row →
   `Stopped { DigestUnverifiable }`. An exact replay of tx 5 and a tx 5 with
   changed records therefore never return the same outcome.
7. A new `Tx` (or a committed chunk) applies its records, its `AppliedTx` and
   the new progress in one local transaction → `Applied`.
8. `Coverage` updates only the progress record → `CoverageAdvanced`, or
   `ProofAccepted` when it is a valid challenge answer (above). A nonce that
   is not outstanding is not a proof; the frame still counts as coverage
   only if its signature verifies.

## 4. Phases against what ships today

- **Now (fixtures and the per-graph feed).** Until the as-of share (s24.1)
  and the scoped replication stream exist, a consumer may drive the applier
  from the shipped per-graph `/changes` pages: one graph per scope, `source`
  from the page's `store_id` and `log_epoch`, no coverage frames. Freshness
  is then `(source_tx, heartbeat age)` with `staleness_bound_ms = None`. A
  browser store built this way keeps the same seam: only the frame source
  changes later. Duplicate handling is NOT relaxed in this phase: the
  applier retains `AppliedTx` and compares digests from the first slice.
- **s24.1:** `bootstrap` from an as-of share, including frozen packs
  (aegis-s24.1.1).
- **s24.2:** the scoped stream with coverage and stream sessions; signed
  coverage through the shared signing crate (S0); the source's answer to a
  challenge (nonce echo, `W` observed after receipt). Until a source answers,
  `challenge` is accepted but no proof can arrive, so freshness stays UNKNOWN.

## 6. Relays (s24.2 S4)

A relay is a replica that serves its own children (`local-sync-mode.md` §6,
"Fan-out"). The master then serves one stream per relay, not one per agent.
A relay uses this same applier upstream and the same producer
(`replicate_source::Session`) downstream. It adds no second applier and no
second frame format.

**Source numbering.** A replica commits each source transaction as a new LOCAL
transaction, so its own change log is numbered locally. Children follow the
SOURCE: their cursor, duplicate digests and coverage are all in source
transaction ids. So the applier records which source transaction each local
transaction it commits belongs to (`replica_tx_map(local_tx, source_tx)`,
written in the same savepoint). A source transaction that spans graphs is
several local transactions, and a bootstrap's local transactions all map to its
watermark. A relay's producer reads its local log and states each frame's
`source_tx` through that map. A local transaction with no map row (one the
relay wrote itself) is never served. A relay refuses local writes anyway, so
such a row means corruption: the session ends with a signed `control` frame
of kind `relay_unmapped`, which the child treats like `tx_too_large`.

**Bootstrap from a relay.** A child bootstraps from its relay's as-of share
(`local-sync-mode.md` §6: "never from a fresh whole-graph export per agent").
The relay cuts at the local transaction mapped to its source cursor. The
manifest states the master's `source` and that source cursor as
`watermark_tx`. A cut between mapped transactions, or below the relay's own
bootstrap watermark, is refused.

**Identity and source.**

- Frames name the MASTER's `(store_id, log_epoch)`, unchanged. A child of a
  relay is a replica of the master, so a restore or epoch change upstream
  reaches every leaf as a source change.
- Session, coverage and control frames are signed by the RELAY's own stream
  key. A child pins its relay's key, never the master's: a coverage frame is
  an assertion by its signer, and the relay is the one asserting what it has
  sent.

**Scope.** A relay serves only a subset of its own scope. A child request is
resolved against the relay's pinned scope lists, never against graphs the
relay merely happens to hold. Read authorization is re-checked at the relay
for every transfer, as at the master (§4 of local-sync).

**Coverage.** A relay's coverage to a child is at most the coverage it has
verified from upstream. A relay holds every in-scope source transaction up to
its upstream `W`, and nothing beyond it. Heartbeats repeat the last coverage.

**Freshness, per hop.** A child's challenge reaching a relay is answered only
from an upstream challenge that the relay sends AFTER the child's challenge
arrived. The upstream answer (`ProofAccepted`, with `W_up`) is relayed as a
coverage frame to the child, echoing the child's nonce. Every transaction up
to `W_up` is sent to the child before that frame. The child's bound,
`now - t_send(child)`, then covers both hops, and no clocks are compared
across machines. A relay whose upstream session is down answers no
challenges, so its children's freshness becomes UNKNOWN.

**Acceptance (S4):**

- A grandchild (master -> relay -> leaf) equals the master in scope.
- A >1 MiB transaction crosses the relay in parts, with nothing visible at the
  leaf before the barrier.
- A leaf challenge yields a bound only after a fresh upstream proof; with the
  upstream down, the leaf stays UNKNOWN.
- An epoch change at the master reaches the leaf as `Rebootstrap`.
- A relay refuses a child scope outside its own.
- The master serves N leaves through one relay with one master stream.

## 5. Acceptance (from local-sync-mode.md §12, P2 row)

Duplicate, reordered and dropped frames; an epoch change; a chunk cut
mid-part; revocation mid-chunk; restart and re-open; a quiet source passing
by coverage; a disconnected replica reporting UNKNOWN freshness. In every
case the replica equals the source at its cursor, or it holds no partial
state and has said what to do next.

Plus the controls these contracts add:

- **Duplicate digest.** An exact redelivery of tx 5 → `Duplicate`; tx 5 with
  one changed record → `Stopped { DigestMismatch }`, nothing written; the
  same tx 5 redelivered as a chunk instead of one frame → `Duplicate`.
- **Challenge age.** Two replicas with the same cursor, coverage, frame and
  `now_ms`, whose proofs answered challenges sent at different `t_send_ms`,
  report different `staleness_bound_ms`; at `now - t_send` of 50 ms and
  500 ms, `within(100)` is true and false respectively.
- **Proof rejection.** A delayed heartbeat (no nonce) leaves freshness
  UNKNOWN; a replayed nonce, a nonce from another session, an unsolicited
  answer and any proof after `open` or `clock_discontinuity` are rejected.
