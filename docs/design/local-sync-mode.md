# Design: Local sync mode — scoped replicas against one master

> **Status (2026-10-16, revision 3 after architecture review): design for
> review. Nothing here is implemented.**
> It records the contract to build, the shipped primitives it reuses, and
> the defects in those primitives that block replication today. Every
> source claim cites the file at `origin/main` 453845ad; this is a historical
> source inventory, not an assessment of current runtime capabilities. The store and
> merge choices marked **OPEN (quipu lead)** are recommendations that still
> need the quipu lead's ruling.

## 1. Goal

Clients do their query, inference and draft-write work on a local copy of
the slice they need. This moves read load and working-set memory off the
server. The loop:

1. **Pull** a scoped slice pinned to a source transaction.
2. **Stay fresh** by tailing committed changes from that transaction.
3. **Work locally** in an overlay on the native or wasm engine.
4. **Push back** a signed changeset. The master merges it three-way
   against the slice as it was at the pull, and conflicts come back as
   results, never as silent overwrites.

The same path serves the HA read replicas: **one replication
path, several profiles** (§3).

Non-goals for this design: automatic failover (HA phase 2, separately
gated), dynamic pattern subscriptions (§4), partial merge apply (§7), and
offline erasure of data a client already holds (§4).

## 2. What already ships, and what blocks replication

| Primitive | Shipped behaviour | Gap for replication |
|---|---|---|
| Change stream | `GET /changes/stream`, one transaction per frame, `graph` filter, tx-id cursor via `since`/`Last-Event-ID` (`src/server/feed_stream.rs:146`, `src/server/event_cursor.rs:5-27`) | See F1–F6 below |
| Event stream | `GET /events/stream`, event-offset cursor, separate number space from tx ids (`feed_stream.rs:189-193,216-217`) | Not a replication feed; this design does not use it for state |
| Stream limits | 64 streams per process, a shared semaphore that answers 503 when full (`feed_stream.rs:87-95`); frame cap 1 MiB (`event_cursor.rs:3,40`) | An oversized transaction sends `quipu.error` **without advancing the cursor** and ends the stream (`feed_stream.rs:203-214`). A consumer reconnecting at the same cursor fails the same way, so it can never get past that transaction. The whole transaction is also materialised in memory first. |
| Share artifact | Manifest carries `store_id`, `tx_anchor`, `graph_hash`, `shapes_hash`, `scope` (root/graph/group/construct), `parent_share` (`src/share.rs:248-334`) | `tx_anchor` is always the latest transaction (`share.rs:478`). There is no as-of cut and no completeness or policy digest. History is not carried (`docs/design/standard-share-artifact.md:194-211`). |
| Share delta | `share-delta/v1` line-set diff against a parent share (`src/share_delta.rs:33-75`) | Deltas are never attested (`share_delta.rs:402-405`) |
| Three-way merge | `share_merge::merge_graphs`, string-compared terms, `sh:maxCount` conflicts as `DecisionRecord`s, all-or-nothing (`src/share_merge.rs:31-38,211-256,293-301`) | Ours is always the whole ROOT. There is no post-merge SHACL gate (`:342`), cardinality ignores target class (`:181-208`), blank nodes are not refused, and valid time and derived facts are ignored. Native only. |
| Fork | Same-store named graph at any tx (`src/store/forks.rs:157-233`) | `fork_promote` is a **two-way overwrite**. It retracts whatever ROOT gained after the fork (`forks.rs:313-353`, `src/store/forks_tests.rs:265-267`), so it is unusable for merge-back. |
| WASM explorer | Loads a share, writes locally under SHACL, emits a delta (`wasm/explorer/src/lib.rs:83-400`) | No attestation verification in wasm (`src/share_import.rs:44-48`). OPFS persistence is proven only in `wasm/harness`. |
| Signing | Ed25519 verdicts; `quipu-write-v1/v2`, `quipu-share-v1` envelopes; durable nonces; bitemporal verifier registry (S1); trust-root gate (S3) (`src/signing.rs`, `src/session_attestation.rs:19-23,95-108,513-543`, `src/governance/verifier_registry.rs`) | There is no shared signing crate yet (S0). `store_id` survives a file copy, so it cannot tell a store from its clone (`docs/design/signing-plane.md:78-81`). |
| Read authorization | Reads are allowed for every caller (`src/http_auth.rs:421-429`); there is no per-graph read check on query, export or either stream | Blocks any multi-tenant slice (§4) |
| Write authority | Per-graph authority along the principal chain, opt-in (`src/governance/authority.rs`, `src/store/gate.rs:379-399`) | Must be on for changeset admission |

**Feed defects that replication would turn into silent divergence.** These
are prerequisites (phase P0, §12). F1, F2 and F6 are findings from reading
the code at 453845ad; P0's first tests must reproduce each one before it is
fixed.

- **F1 — functional-property supersede emits no retract.** Under the
  `owl` feature, prior values are closed with an `UPDATE` and no op=0
  record (`src/store/gate.rs:144-146`, called from `src/store/ops.rs:288-290`).
  A feed replica would keep both values. The in-process read-model catch-up
  handles this case (`src/store/reads.rs:74-91`); the feed does not.
- **F2 — deep freeze deletes served history.** `DELETE FROM main.facts
  WHERE g = ?` (`src/store/freeze.rs:212`) removes rows a cursor may
  already have read. A replay from an older cursor silently returns fewer
  records.
- **F3 — no valid time in change records** (`src/store/changes.rs:205-215`).
- **F4 — byte values are lossy**, reduced to `{"bytes_len": n}` (`changes.rs:278`).
- **F5 — no source identity in pages.** `store_id` is absent from change
  pages (`changes.rs:89-97`), and there is no log epoch, so a follower
  cannot detect a restored or cloned primary.
- **F6 — the oversized-transaction dead end** described in the table.

## 3. Roles: one replication path, several profiles

There is one bootstrap-and-tail **applier**, one cursor and recovery
implementation, and one compatibility matrix. The HA follower work builds it. The client profile adds the
overlay and admission on top. Each profile is a configuration of that
applier:

| Profile | Holds | Serves | Writes | May relay | Promotable |
|---|---|---|---|---|---|
| **Master** | the authoritative store | everything | orders all accepted writes | yes | n/a |
| **Full follower** (HA) | every graph, canonical, read-only | reads within a declared lag | refuses every mutation (positive control in the HA profile) | yes | phase 2 only, behind the fenced epoch (§9) |
| **Scoped follower** | a declared graph set, canonical, read-only | reads within its scope | refuses | yes, for a subset of its scope | **never** |
| **Client** | a scoped follower base plus a local overlay | local reads; base reads are tagged with freshness | local overlay only; changesets go to the master | no (MVP) | **never** |

Rules:

- **Only the master orders accepted writes.** A follower or client never
  re-stamps a transaction id.
- **A partial replica cannot become a master by accident.** Promotion
  requires a replica manifest whose scope is `full`, a matching
  `(store_id, log_epoch)`, and a caught-up watermark. Any scoped manifest
  refuses promotion structurally, not by convention.
- HA followers are the full-follower profile of this applier. The HA design should
  link to this contract rather than build a second feed applier.

## 4. Replication unit and scope

**MVP unit: a named-graph set.** A slice is a set of graph IRIs. The change
stream already filters by graph and every record names its graph
(`changes.rs:212`). Dynamic CONSTRUCT or pattern slices are deferred. Pattern
membership changes (a fact stops matching and must leave the replica)
need their own deletion contract, or pattern replicas would silently
keep stale data.

Scope is **authorization and semantic completeness at once**:

- **Read authorization is a prerequisite** (P1). Snapshot, export and
  subscribe must all enforce the same authenticated per-graph read policy,
  metadata included. Every relay enforces it again for its own children.
  Today no read route checks anything (§2).
- **Authorization is checked at every transfer, not only at subscribe
  time.** The source and every relay re-check the principal's current grant
  for the scope at each of these points:
  - building or serving an as-of share, including one from cache;
  - emitting each `tx` frame or chunk part;
  - accepting a reconnect or resume.

  A resume token or cached share is bound to `(principal, scope_digest,
  policy_digest, log_epoch)` and is re-authorized when it is used. A graph
  hash, or possession of a cached share, is never authority.
- **Revocation travels on the control channel.** It is delivered as a signed
  `control` frame on every open session for that principal (§5), which is
  never graph-filtered. It is also returned in place of data on the next
  transfer attempt. Revocation mid-chunk discards the stage. Revocation
  between bootstrap and tail refuses the tail.
- **Offline serving is lease-bound.** A client holds a grant lease with an
  expiry, renewed while connected:
  - Before expiry, it may serve the scope locally.
  - After expiry, or once it has seen a revocation, it **fails closed**
    for any other principal, such as a relay child or a local service
    acting for another user.
  - Its own reads of data it already holds stay available, labelled
    **advisory: authority unverified since t**.

  Data already copied to an offline device cannot be remotely erased. The
  guarantee is "no new data after revocation or lease expiry, and no serving
  to others", not erasure.
- **Shape and policy context crosses slices.** SHACL class and inverse
  constraints, inference rules and policy may reference graphs outside the
  slice. A slice manifest declares its context graphs. Local validation of a
  partial slice is **advisory**: it can never certify global conformance.
  The master's validation in the accepting transaction is authoritative (§7).
  A local query that needs undeclared context answers **UNKNOWN**. There
  is no implicit ROOT fallback.

## 5. Bootstrap and the snapshot-to-stream cut

**Bootstrap artifact: an as-of share.** This is new: a share cut at a
chosen transaction `T` and scoped to the graph set. The cut is one
consistent snapshot read at `T`. Its manifest adds:

- `source = {store_id, log_epoch}`. `log_epoch` is new (F5). It is minted at
  store creation and re-minted by any history-removing operation such as
  deep freeze (F2). It is also re-minted by an **explicit ownership step**:
  `quipu store restore` and `quipu store adopt` mint a new epoch and record
  it in an ownership file bound to the host and the data path. A store
  opened at a different path or host than its ownership record refuses to
  serve replication or accept writes until an operator runs `adopt`. That
  is what prevents two copies from serving the same epoch; clone detection
  alone does not.
- `watermark_tx = T`, the last source transaction included.
- `scope_digest` (the sorted graph IRIs plus declared context graphs) and a
  `policy_digest`.
- `shapes_hash` and the shape-set versions (`docs/design/shape-versioning.md`).
- `completeness`, which must cover valid time (F3) and lossless bytes (F4).
  A bootstrap that cannot carry them is refused, not degraded.

**Tail: one stream for the whole graph set.** The shipped `graph` filter
takes one graph (`src/server/feed_stream.rs:72-86`). Tailing N graphs as N
streams would split a multi-graph transaction across streams, with no
atomicity between them. The replication stream therefore takes the scope
itself: `POST /replicate/stream` with the scope's graph lists and `since=T`
(a POST, like `/share/asof`, because graph IRIs do not belong in a query
string; the session frame states the `scope_digest`). Each frame
carries every in-scope record of one source transaction, across all its
graphs, and is applied as one unit.

**Frames.** Every frame is bound to a **stream session**: the source
`(store_id, log_epoch)`, the `scope_digest`, the authenticated principal
and a session id. Each frame carries a strictly increasing `frame_seq`
within that session. There are three kinds:

- `tx`: the in-scope records of one source transaction, or its chunk
  header (§6).
- `coverage`: a **source-issued high-watermark** saying "every source
  transaction ≤ `W` that touches this scope, in this epoch, has been sent
  in this session". It is emitted after every `tx` frame, and on a fixed
  heartbeat while the source is idle or every transaction is filtered out.
  It is independent of whether any changes were returned.
- `control`: revocation, epoch end, and protocol errors (§4). It is never
  filtered by graph.

`coverage` and `control` frames are signed by the source's stream key,
using the shared crate (S0). A relay forwards them unchanged and emits its
own signed coverage for its own children. A coverage frame is an
assertion by its signer, not an inference from silence.

**Applying.** One local transaction applies a `tx` frame's facts, any
registry or metadata rows, and the progress record. The progress record
holds the cursor `(store_id, log_epoch, source_tx)`, the last verified
coverage `W` and the last `frame_seq`. Coverage frames update the progress
record alone. The source tx is persisted as provenance and is never
confused with the local tx or an event offset.

| Case | Required outcome |
|---|---|
| Crash before the local commit | No facts and no progress change; resume from the stored cursor in a new session |
| Crash after the local commit | Progress already advanced; resume cleanly |
| **Duplicate**: a new session redelivers `source_tx` ≤ cursor | Verify its digest equals the applied one, then skip and count it. A mismatch stops the applier. |
| **Reorder** within a session: `frame_seq` not exactly last+1, or `source_tx` ≤ cursor | Stop, discard the session, reconnect from the cursor. This is a transport fault, not data. |
| **Dropped frame** | Detected by the `frame_seq` gap, or by a coverage `W` ahead of the cursor with no `tx` frame for an in-scope transaction. Never inferred from tx-id gaps. |
| Gap in tx ids | Normal. Out-of-scope transactions are skipped (`src/store/changes.rs:116-117`). Completeness comes only from signed coverage. |
| **Quiet source** (no commits for an hour) | Heartbeat coverage keeps proving completeness at `W`. Freshness comes from a challenge answer (below), not from the heartbeat. |
| **Disconnected follower** | No challenge is answered, so no valid proof exists. Freshness is UNKNOWN and bounded reads refuse. It must never report "current". |
| **Filtered no-op** (source busy, nothing in scope) | Coverage advances `W` with no `tx` frames |
| **Dropped matching tx** (an in-scope tx omitted, coverage still sent) | Coverage claims `W` ≥ that tx while the follower never applied it. Detected and refused when the follower audits coverage against the as-of digest (P1 test fixture). A source that signs false coverage is a source fault; the signature makes it attributable. |
| `log_epoch` or `store_id` mismatch | Stop. Canonical state is rebuilt from a fresh as-of share. The overlay is handled by §7's stale-base rules, never retargeted. |
| Cursor below a retention floor | The change log has no GC today (`changes.rs:30-31`). Any future floor must answer an explicit "cursor too old", forcing a re-bootstrap, never a silent resume. |
| Transaction over budget | §6 |

**Freshness needs a challenge, not a heartbeat.** A signature proves who
sent a frame and what it says, not when. A valid heartbeat delayed 60 s in a
queue arrives in order and correctly signed. Heartbeats and coverage frames
therefore prove **completeness** (everything ≤ `W` was sent), never
**freshness**, and never reset a freshness clock.

Freshness is proven by a same-clock challenge:

1. The follower sends a challenge on the session. It carries an
   unpredictable 128-bit nonce bound to the session id, and the follower
   records its own monotonic time `t_send`.
2. The source answers with a signed coverage frame that echoes the nonce.
   It states `W` as observed **after** it received the challenge.
3. When the follower receives it, it verifies the signature, the nonce and
   the session. The coverage was then true at some instant in
   `[t_send, t_recv]`, so **staleness ≤ now − `t_send`** for as long as the
   cursor is at or past `W`. This is a conservative upper bound that
   includes every queueing and network delay. No delivery time is assumed.

Rules:

- A proof is valid only for its session and only while the follower's
  monotonic clock is continuous. A process restart or a detected suspend
  discards it. Cached, replayed or unsolicited coverage is never a proof.
- **Per hop.** A relay answers a child's challenge only with coverage from an
  upstream challenge that it issued **after** the child's challenge
  arrived. The bound at the leaf is still `now − t_send(leaf)`, and it
  covers every hop by construction. Hop ages are never added or compared
  across clocks.
- Followers may challenge periodically. A `bounded=<ms>` read (§9) is
  satisfied only if `now − t_send` of the latest valid proof is at most ms.
- With no valid unexpired proof, freshness is **UNKNOWN**: a bounded read
  refuses or falls back to the primary. It never uses a guessed delivery
  time.
- The upstream commit timestamp is carried for provenance only.

**Controls (P2):**

- Delay a valid next heartbeat by 60 s, with no loss or reorder: a
  `bounded=100ms` read **refuses**.
- A fresh challenge answered within 100 ms **passes**.
- A quiet source passes through a challenge, with no commits.
- A proof is rejected after a restart, or with a replayed nonce.

## 6. Bounded transfer, recovery and relay

**Configurable limits.** The values are deployment configuration, not
numbers this design guesses. Every limit has a hard refusal outcome and a
metric.

| Limit | Applies to | When exceeded |
|---|---|---|
| `part_max_bytes`, `part_max_rows` | one chunk | the producer splits further; never exceeded |
| `tx_max_bytes`, `tx_max_rows` | one source transaction | `tx_too_large` control frame. The consumer stops; recovery is a re-bootstrap from an as-of share at or after that tx. The cursor is unchanged. |
| `stage_quota_bytes` per consumer | on-disk staging | `stage_quota_exceeded`. The partial stage is discarded; cursor unchanged. |
| `max_concurrent_stages` | per consumer | at most one in-flight transaction per session |
| `stage_timeout` | an unfinished stage | stage discarded; reconnect resumes from the cursor |
| `max_concurrent_exports` | source and each relay | as-of share builds queue; excess requests get 503 with retry-after |
| `share_cache_quota_bytes`, `share_cache_ttl` | cached as-of shares | LRU eviction. Only a **latest-bootstrap** request may be served from a newer cached `T'`, with an explicit manifest naming `T'`. An **exact as-of** request, such as an overlay base or a historical cut, is never substituted: it is rebuilt under the export limit, or refused. A cache entry is never pinned forever. |
| `max_streams` (64 today) | source | 503; jittered exponential backoff, never a tight retry |
| `offline_max` | client | after this duration offline, the client must re-bootstrap canonical state; its overlay is kept (§7) |

**Chunked transactions.** A transaction over `part_max_*` is sent as:

1. a header carrying `source_tx`, `parts = n`, total bytes, total rows and
   the whole-transaction digest. The producer computes these in a bounded
   pre-pass that counts rows; it does not hold them in memory. If the
   header already exceeds `tx_max_*`, the source sends `tx_too_large`
   instead.
2. `n` parts, each with its index and a running digest;
3. a commit barrier that repeats the whole-transaction digest.

The consumer writes parts to a staging table under `stage_quota_bytes`. It
applies the staged transaction and advances progress in one local
transaction, and only after the barrier digest verifies. Interruption,
oversize, quota, timeout or digest mismatch discard the stage. Every
restart replays the transaction from part 1. Partial stages are never
resumed, so no partial transaction is ever visible.

**Acceptance cases.**

- A transaction larger than 1 MiB goes primary → relay → leaf, with no
  partial visibility and no cursor advance before the barrier.
- Each of disconnect mid-part, `stage_quota_exceeded`, `tx_too_large` and
  a corrupted part leaves **zero** visible facts and an **unchanged**
  cursor, with no reconnect storm.
- Peak producer, relay and consumer memory and disk are measured against
  the configured limits.

**OPEN (quipu lead):** chunking, versus an authenticated "re-pull the as-of
share at tx N" fallback as the only big-transaction path.

**Fan-out.** Relays form a tree, so the master's stream cap counts one
stream per relay, not per agent. **Bootstrap cost:** a new agent bootstraps
from its relay's cached as-of share plus a short tail, never from a fresh
whole-graph export per agent.

## 7. Overlay and merge-back

**Client layout.** A client keeps three things:

- **Canonical state.** Only the applier writes it, and it keeps advancing
  from the tail.
- **The overlay.** A local fork-like graph that holds proposals.
- **A base journal.** When the overlay is opened, the client freezes an
  immutable copy of the slice it touches, as of `base_tx`. It is stored as
  canonical N-Quads that include the graph and valid time, using RDFC-1.0
  as shares already do (`src/share.rs:493`). RDFC-1.0 canonicalizes quads
  only, so valid time is bound separately: each quad's canonical N-Quad
  line is paired with its `valid_from` and `valid_to` (or `open`), the
  pairs are sorted, and `base_slice_hash` is the SHA-256 over that list.
  It is not the RDFC-1.0 hash alone.

The base journal is retained until every changeset built on it resolves
(accepted, refused, or superseded by a rebase). Canonical state advancing,
or being rebuilt after an epoch change, never touches it. **Local inference**
writes into a separate derived graph that is never included in a changeset,
so derived facts are never proposed as assertions.

**Changeset** = ordered N-Quads asserts and retracts with explicit graphs,
plus `base = {store_id, log_epoch, base_tx, base_slice_hash}`. Optional
valid-time intervals may be proposed. Transaction time is always the
master's.

**Stale bases are explicit refusals, never retargets:**

| Condition at admission | Outcome |
|---|---|
| `base.log_epoch` ≠ the master's current epoch | `foreign-epoch` refusal |
| Master cannot reconstruct the as-of base, for example because a deep freeze removed it | `base-unavailable` refusal |
| Reconstructed base digest ≠ `base_slice_hash` | `base-mismatch` refusal (client bug or tamper) |

In every case nothing is written, and the pending work stays in the
overlay and base journal.

**Rebase workflow.** The client bootstraps fresh canonical state. It runs a
local three-way merge (old base journal → new base, overlay on top) and
records conflicts the same way the master would. It opens a new base journal
and signs a **new** changeset with a **new** `operation_id`. The old
operation is marked superseded locally. An old changeset is never re-signed
or silently pointed at a new base.

**Admission on the master is one transaction:**

1. Authenticate the signature (§8) and check write authority for every
   target graph against **current** authority.
2. Three-way merge, reusing `share_merge`'s string-term semantics
   (sound across independent dictionaries for IRIs and literals):
   - **base** is an as-of read of the slice at `base_tx`;
   - **ours** is the current master slice, scoped to the same graphs rather
     than the whole ROOT;
   - **theirs** is base plus the client's delta.
3. Required extensions to the shipped merge:
   - Cardinality is resolved per target class, not per predicate.
   - Blank nodes are refused unless skolemized, as `git_merge` already
     refuses them (`src/git_merge.rs:150-160`).
   - Delete-versus-update on a functional slot is a conflict.
   - A proposed valid-time interval that overlaps a newer master interval
     is a conflict.
4. **Full SHACL on the complete candidate against the master's current
   shapes**, inside the same accepting transaction. The client's older
   shapes are never trusted.
5. Any conflict, or any UNKNOWN context, means nothing is written. The
   receipt returns `DecisionRecord`s (`share_merge.rs:31-38`). The client
   resolves them through the shipped emit/propose/decide flow
   (`src/share_merge_decisions.rs`) and submits a new changeset. Partial
   apply is out of MVP.

`fork_promote`'s two-way overwrite is **not** used for client merge-back.

**Replication versus admission.** Applying an already-accepted upstream
commit on a follower is replay. It preserves the original provenance and
transaction boundaries. It does not re-validate against new shapes, and it
does not pretend old commits were checked against them. Admitting a new
client proposal is the gated path above. These are two code paths with
different guarantees, and they must not share an entry point.

**OPEN (quipu lead):** reuse of `merge_graphs` versus a new store-level
merge; and whether the base read at `base_tx` stays affordable or needs
the client to send `base_slice_hash` plus the master's cached as-of share.

## 8. Signed changesets

This extends the signing plane (`docs/design/signing-plane.md`) and
requires the shared signing crate first (S0). Yupana mirrors `signing.rs`
by doc comment today, and clients would make a third copy.

**Envelope `quipu-changeset-v1`.** It is domain-separated and uses the
canonical `key=value` form of the session attestation
(`src/session_attestation.rs:513-543`). It binds:

- `audience`, the master's `(store_id, log_epoch)`, and `role=client`;
- `scope_digest`;
- `base = (store_id, log_epoch, base_tx, base_slice_hash)`;
- `delta_sha256` over the ordered N-Quads;
- any valid-time claims;
- `key_id`, `operation_id` (UUID), `issued_at` and `nonce`.

**Client keys** are registered as facts, like `VerifierRegistration`, with
a write scope of graphs and predicates. Only master-side authority may
create a registration, through the trust-root gate (S3). **A client never
registers its own authority.** Trust is a query: the signature is valid,
the key is registered at the merge transaction, and the key is authorized
for every touched graph. A valid signature alone is neither authorization
nor truth.

**Resolver signature.** A changeset that resolves conflicts carries a
second signature. It binds both parents (`base_tx` and the master tx it
was resolved against), the hash of the decision records, and the
validation evidence.

**Receipt.** The accepting transaction writes a `ChangesetReceipt`. It
links `operation_id` to the accepted tx, the signer and the verdict.
Contributed facts link to the changeset through that transaction's
provenance.

**Idempotency.** The same `operation_id` with the same digest returns the
same receipt, so a lost response is retried and read back safely. The same
`operation_id` with a different digest is refused.

**Bitemporal trust.** "Trusted when merged" and "trusted today" are
separate queries over the shipped bitemporal registry (S1). Revocation
closes the registration's valid time and never rewrites past receipts.
Keys stay outside the store (signing-plane §4).

**Clone identity.** `store_id` cannot tell clones apart. Clients therefore
identify by registered `key_id`, and the master identifies by
`(store_id, log_epoch)`, with `log_epoch` re-minted on restore or detected
clone (F5).

## 9. HA reads and authority (HA boundary)

- **Phase 1 followers are read-only.** Failover is phase 2 and separately
  gated.
- **Per-request consistency** (staleness as defined in §5). A read may declare `eventual`,
  `bounded=<ms>`, or `min-tx=<store_id:log_epoch:tx>` (read-your-write after a
  receipt). The router picks a follower that satisfies it, else the
  primary, else answers 503 with UNKNOWN freshness. It never answers from a
  stale replica without saying so.
- **Every response reports its source watermark**:
  `(store_id, log_epoch, applied_tx, that commit's timestamp)`. Transaction
  distance alone is not time lag.
- **Endpoints are classified by side effect, not by HTTP method.**
  `POST /query` is a read. `/events/commit` mutates consumer state and goes
  to the primary.
- **Rolling N/N+1 upgrades** need a stream protocol version in the
  handshake, plus database, schema and shape compatibility across one
  version. Drain → upgrade → catch up → rejoin is verified per member.
- **Phase 2 (requirements only):** a fenced authority epoch that every
  mutation checks. The host single-writer guard is not a distributed lease.
  The old primary is demoted and re-bootstrapped, and partitions are
  handled without split brain. Idempotent writes survive a primary outage
  by `operation_id`.

## 10. Stack components as clients

| Component | Profile and scope | Freshness need | Offline writes | Signing key / read credential | Bootstrap and rejoin |
|---|---|---|---|---|---|
| **Seeds** | client: the agent's board graphs | bounded, seconds; plate reads tagged | **No.** Claims, assignment and outbox receipts stay master-atomic. An offline plate read must never authorize a claim. | signing key on host; read credential for its scope | relay share plus tail |
| **Yupana** | client: policy-projection graphs | every verdict states freshness | signed verdicts promoted as changesets | signing key (shared crate, S0); read credential | rebuilt on this loop (below) |
| **Chaski** | scoped follower | bounded | none | no signing key; read credential required | already a stream consumer; point it at a relay |
| **st agents** | client: plate, policies, recent context | bounded | proposals only | per-agent signing key; read credential | relay share plus tail |
| **Bobbin** | scoped follower | eventual | none | no signing key; read credential required | index from the change stream, not polling |
| **Tambo** | provisions replicas | n/a | n/a | installs signing keys and read credentials | proves the loop: a marker written locally, merged, and visible on the master |
| **Browser (wasm)** | client | eventual | proposals only | **unsupported until** wasm attestation verification and key custody exist (§2) | OPFS persistence first |

**Yupana as a client.** It is rebuilt on this protocol, not used as its
base. These behaviours are acceptance criteria:

- a one-way cache;
- invalidation on writes;
- freshness stated on every verdict;
- loud fail-open, with explicit UNKNOWN;
- signed verdicts promoted back as changesets.

Only advisory evaluations may fail open, and each one is named. A failed
authority lookup is UNKNOWN, never "ungoverned". No existing enforcement
gate changes.

## 11. Operator rollback

Rollback never rewinds accepted master history.

- **Withdraw routing.** Remove replicas from read routing, so reads go to the
  master again, and stop relays. Clients fall back to master reads.
- **Keep the record.** Proposal journals, base journals, `ChangesetReceipt`s
  and accepted transactions are all retained. A rolled-back client loses
  no pending work.
- **Rebuild only caches.** Follower and client canonical state is
  disposable and is rebuilt from an as-of share when replication resumes.
- **Admission off.** The changeset admission endpoint can be disabled on its
  own; clients keep their overlays until it returns.

## 12. Phasing and proof

Each phase is a separate source change, merge, runtime activation and
rollback.

| Phase | Deliverable | Acceptance (falsifiable) |
|---|---|---|
| **P0** feed correctness | F1–F5: supersede retracts, freeze epoch bump, valid time, lossless bytes, `store_id`/`log_epoch` on pages | A replica built from the feed equals an as-of read for every graph in the conformance fixtures, including OWL supersede and freeze. Mutation tests fail without each fix. The `adopt`/`restore` ownership step refuses a copied store until run. |
| **P1** cut and authz | as-of scoped share with the §5 manifest; per-graph read authorization on query, export and stream | Bootstrap at `T` under concurrent writes followed by a tail from `T` has no gap or overlap. A denied graph is refused on every route, metadata included (positive and negative controls). Revocation refuses a cached share and a resume token; an expired lease fails closed for a second principal. |
| **P2** follower (= HA phase 1) | the applier, full and scoped profiles, chunked frames | duplicate, reordered and dropped frames; freshness challenge controls (§5: 60 s-delayed heartbeat refuses `bounded=100ms`, fresh challenge passes, restart and replayed nonce rejected); quiet source passes by challenge; disconnected follower goes UNKNOWN; filtered no-op; dropped matching tx detected; a >1 MiB tx through a relay; zero visible facts and unchanged cursor on disconnect mid-part, quota, oversize and corrupt part; revocation mid-chunk; restart, restore and epoch change; a partial replica's promotion refused; 0 failed reads across a rolling deploy; lag bound with an alert |
| **P3** client | overlay, changeset envelope, admission, receipts | offline proposal → master receipt → all replicas converge; stale shapes and conflicting edits return DecisionRecords; `foreign-epoch`, `base-unavailable` and `base-mismatch` refused with the overlay intact, then rebased under a new `operation_id`; tamper, replay and revoked-key cases refused; identical retries return the same receipt |
| **P4** stack | Seeds plates, Yupana rebuild, Bobbin indexing, Tambo marker round-trip | master read load and client footprint measured before and after; UNKNOWN and no-false-zero controls |
| **P5** browser | wasm verification, key custody, OPFS | explicit support boundary; no assumed parity |

**Hard phase gates.** Merging this document approves the direction only.
These rulings must exist before the phase that depends on them starts:

- the quipu lead's store and merge rulings (§13 item 1) before P0 and P2;
- the read-authorization model (P1) before any scoped follower or client
  serves a second principal;
- the S0 shared signing crate before P3.

Merging this document does not count as any of those rulings.

The proposed P1 model is detailed in
[per-graph read authorization](graph-read-authorization.md). That proposal
keeps the temporary bootstrap protection in place until its implementation
and paired authorization controls are admitted.

Benchmarks and production replays need their own authorization. None are
granted by this document.

## 13. Open questions

1. **OPEN (quipu lead):** the six store and merge recommendations in this
   design: named-graph unit; F1/F2 fix by emitting records rather than
   refusing; chunking over re-pull; `log_epoch`; `share_merge`-based admission
   with a post-merge SHACL gate; one applier shared with HA followers.
2. Valid time on client edits: proposed valid time accepted under the §7
   conflict rule, or master-stamped only?
3. Clients offline across a shape change: re-validate the overlay locally
   on rejoin (advisory) and always on the master (authoritative). Is a
   shape-version mismatch a hard refusal or a conflict?
4. Who may pull which slice: this depends on the read-authorization model
   (P1) and on `docs/design/group-isolation.md`, which is still deferred.
5. Event retention (`[quipu.events] retention_days`) does not apply to the
   change log. Confirm that no change-log GC is planned without the
   "cursor too old" contract in §5.
