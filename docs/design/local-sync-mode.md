# Design: Local sync mode — scoped replicas against one master

> **Status (2026-10-16): design for review. Nothing here is implemented.**
> It records the contract to build, the shipped primitives it reuses, and
> the defects in those primitives that block replication today. Every
> source claim cites the file at `origin/main` 453845ad. The store and
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

The same path serves the HA read replicas of aegis-s34: **one replication
path, several profiles** (§3).

Non-goals for this design: automatic failover (s34 phase 2, separately
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
are prerequisites (phase P0, §11). F1, F2 and F6 are findings from reading
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
implementation, and one compatibility matrix. s34 builds it. s24 adds the
overlay and admission on top. Each profile is a configuration of that
applier:

| Profile | Holds | Serves | Writes | May relay | Promotable |
|---|---|---|---|---|---|
| **Master** | the authoritative store | everything | orders all accepted writes | yes | n/a |
| **Full follower** (s34) | every graph, canonical, read-only | reads within a declared lag | refuses every mutation (positive control in s34) | yes | phase 2 only, behind the fenced epoch (§9) |
| **Scoped follower** | a declared graph set, canonical, read-only | reads within its scope | refuses | yes, for a subset of its scope | **never** |
| **Client** | a scoped follower base plus a local overlay | local reads; base reads are tagged with freshness | local overlay only; changesets go to the master | no (MVP) | **never** |

Rules:

- **Only the master orders accepted writes.** A follower or client never
  re-stamps a transaction id.
- **A partial replica cannot become a master by accident.** Promotion
  requires a replica manifest whose scope is `full`, a matching
  `(store_id, log_epoch)`, and a caught-up watermark. Any scoped manifest
  refuses promotion structurally, not by convention.
- s34's followers are the full-follower profile of this applier. s34 should
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
- **Revocation.** A client learns of a grant revocation from a `scope.revoked`
  record on its stream. It then stops serving the revoked graphs and drops
  them at its next open. Data already copied to an offline device cannot be
  remotely erased. The guarantee is "no new data after revocation, and the
  data stops being served once the client sees the record", not erasure.
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

- `source = {store_id, log_epoch}`. `log_epoch` is new (F5): it is minted at
  store creation and re-minted on restore, on clone detection, and on any
  history-removing operation such as deep freeze (F2).
- `watermark_tx = T`, the last source transaction included.
- `scope_digest` (the sorted graph IRIs plus declared context graphs) and a
  `policy_digest`.
- `shapes_hash` and the shape-set versions (`docs/design/shape-versioning.md`).
- `completeness`, which must cover valid time (F3) and lossless bytes (F4).
  A bootstrap that cannot carry them is refused, not degraded.

**Tail:** `GET /changes/stream?graph=…&since=T`. The applier enforces these
rules:

- One local transaction applies a frame's facts, any registry or metadata
  rows, and the cursor `(store_id, log_epoch, source_tx)` together.
- The source tx is persisted as provenance. It is never confused with the
  local tx or with an event offset.

| Case | Required outcome |
|---|---|
| Crash before the local commit | No facts and no cursor change; resume from the old cursor |
| Crash after the local commit | Cursor is already advanced; the next frame resumes cleanly |
| Duplicate frame (`source_tx` ≤ cursor) | Skip; count it in a metric |
| Out-of-order frame (`source_tx` decreases within an epoch) | Refuse and stop the applier; this is a protocol fault, not data |
| Gap in tx ids under a graph filter | Normal. Filtered transactions are skipped (`changes.rs:116-117`). A gap is never treated as lost data. Completeness comes from the page watermark, not from contiguity. |
| `log_epoch` or `store_id` mismatch | Stop and re-bootstrap from a fresh as-of share. Never apply a foreign sequence. |
| Cursor below a retention floor | The change log has no GC today (`changes.rs:30-31`). Any future floor must answer an explicit "cursor too old", which forces a re-bootstrap. It must never silently resume. |
| Frame over the size budget | Chunked delivery (§6) |

**Through a relay**, upstream identity survives unchanged: each frame keeps
the upstream `(store_id, log_epoch, source_tx)` and the upstream commit
timestamp. Wall clocks are **not** compared across hops. A relay states
freshness as "upstream watermark and that commit's timestamp", plus its
own lag measured on its own clock.

## 6. Bounded transfer, recovery and relay

- **Chunked transactions (recommended over re-pull).** The producer streams
  a large transaction's rows from SQLite in bounded parts, each tagged
  `(source_tx, part i of n, running digest)`. It does not materialise the
  whole transaction. The consumer stages parts on disk. It applies them and
  advances the cursor only after the final part verifies, so a partial
  transaction is never visible. This replaces the F6 dead end. **OPEN
  (quipu lead)** whether chunking or an authenticated "re-pull the as-of
  share at tx N" fallback is preferred. Either one must bound producer
  memory, not only client memory.
- **Acceptance case:** a transaction larger than 1 MiB goes primary → relay
  → leaf with no partial visibility, no cursor advance before commit, and
  no reconnect or retry storm. Peak producer and consumer memory is
  measured.
- **Fan-out.** Relays form a tree, so the master's 64-stream cap counts one
  stream per relay, not per agent. A 503 at capacity means jittered
  exponential backoff, never a tight retry.
- **Bootstrap cost.** As-of shares are cached by `(scope_digest, T)` on the
  master and on relays. A new agent bootstraps from its relay's cached
  share plus a short tail, never from a fresh whole-graph export per agent.
- **Offline duration** is unbounded while the epoch is unchanged. An epoch
  change forces a re-bootstrap; the overlay survives as a pending
  changeset (§7).

## 7. Overlay and merge-back

**Client layout.** A client keeps a canonical **base**, which only the
applier writes, plus an **overlay**, a local fork-like graph that holds
proposals. The base keeps advancing from the tail. The overlay records the
base `(store_id, log_epoch, base_tx)` it was opened against. **Local
inference** writes into a separate derived graph that is never included in
a changeset, so derived facts are never proposed as assertions.

**Changeset** = ordered N-Quads asserts and retracts with explicit graphs,
plus `base = {store_id, log_epoch, base_tx, base_slice_hash}`. Optional
valid-time intervals may be proposed. Transaction time is always the
master's.

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

## 9. HA reads and authority (s34 boundary)

- **Phase 1 followers are read-only.** Failover is phase 2 and separately
  gated.
- **Per-request consistency.** A read may declare `eventual`,
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

| Component | Profile and scope | Freshness need | Offline writes | Key custody | Bootstrap and rejoin |
|---|---|---|---|---|---|
| **Seeds** | client: the agent's board graphs | bounded, seconds; plate reads tagged | **No.** Claims, assignment and outbox receipts stay master-atomic. An offline plate read must never authorize a claim. | agent key on host | relay share plus tail |
| **Yupana** | client: policy-projection graphs | every verdict states freshness | signed verdicts promoted as changesets | yupana key; shared crate (S0) | rebuilt on this loop (below) |
| **Chaski** | scoped follower | bounded | none | n/a | already a stream consumer; point it at a relay |
| **st agents** | client: plate, policies, recent context | bounded | proposals only | per-agent key | relay share plus tail |
| **Bobbin** | scoped follower | eventual | none | n/a | index from the change stream, not polling |
| **Tambo** | provisions replicas | n/a | n/a | installs keys | proves the loop: a marker written locally, merged, and visible on the master |
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

## 11. Phasing and proof

Each phase is a separate source change, merge, runtime activation and
rollback.

| Phase | Deliverable | Acceptance (falsifiable) |
|---|---|---|
| **P0** feed correctness | F1–F5: supersede retracts, freeze epoch bump, valid time, lossless bytes, `store_id`/`log_epoch` on pages | A replica built from the feed equals an as-of read for every graph in the conformance fixtures, including OWL supersede and freeze. Mutation tests fail without each fix. |
| **P1** cut and authz | as-of scoped share with the §5 manifest; per-graph read authorization on query, export and stream | Bootstrap at `T` under concurrent writes followed by a tail from `T` has no gap or overlap. A denied graph is refused on every route, metadata included (positive and negative controls). |
| **P2** follower (= s34 phase 1) | the applier, full and scoped profiles, chunked frames | duplicate, reordered and dropped frames; a >1 MiB tx through a relay; restart, restore and epoch change; permission loss at the relay; a partial replica's promotion refused; 0 failed reads across a rolling deploy; lag bound with an alert |
| **P3** client | overlay, changeset envelope, admission, receipts | offline proposal → master receipt → all replicas converge; stale shapes and conflicting edits return DecisionRecords; tamper, replay and revoked-key cases refused; identical retries return the same receipt |
| **P4** stack | Seeds plates, Yupana rebuild, Bobbin indexing, Tambo marker round-trip | master read load and client footprint measured before and after; UNKNOWN and no-false-zero controls |
| **P5** browser | wasm verification, key custody, OPFS | explicit support boundary; no assumed parity |

Benchmarks and production replays need their own authorization. None are
granted by this document.

## 12. Open questions

1. **OPEN (quipu lead):** the six store and merge recommendations as filed
   on aegis-s24: named-graph unit; F1/F2 fix by emitting records rather than
   refusing; chunking over re-pull; `log_epoch`; `share_merge`-based admission
   with a post-merge SHACL gate; one applier shared with s34.
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
