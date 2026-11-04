# Per-graph writable stores

Status: proposed design, not implementation or migration authority.

Give each routed named graph its own SQLite file, writer owner and bounded read
pool. Keep graph and entity IRIs unchanged. Compose reads across those owners
without turning unrelated graph writes into a single global queue. Use Seeds as
the first application pilot, after preserving its existing multi-graph atomicity.
No database split, new process, ownership change or production experiment is
part of this document.

## Ground and problem

This design is grounded in Quipu revision `11764d3a`:

- [StoreHandle](../../src/server/handle.rs) owns one writer mutex and one read
  pool for a Store. More readers do not create another SQLite writer.
- [Attachments](../../src/store/attach.rs), [term aliases](../../src/store/alias.rs)
  and [multi-DB composition](multi-db-composition.md) already compose reads.
  The supported attachment path is read-only. Local writes into an attached
  graph must still refuse; changing a mount flag is not a writable router.
- [Graph batches](../../src/store/batches.rs) use an outer savepoint to preserve
  all-or-nothing writes across graphs in one file. The ordinary authority,
  ownership, SHACL and policy pipeline still runs for each graph.
- [Attachment scope](../../src/sparql/attach_scope.rs) distinguishes proven
  local-only queries from composed queries. Existing scalar transaction-time
  behavior does not supply an ordering across separate files.
- [The provider seam](../../src/provider/mod.rs) is not a distributed SPARQL
  join implementation. Concatenating remote result sets cannot replace local
  composition, join semantics or dataset authorization.

Seeds revision `46c05b1e2f1452069abd2bdce667d92846de960c`,
[`QuipuBackend::commit`](https://github.com/intigraph/seeds/blob/46c05b1e2f1452069abd2bdce667d92846de960c/src/quipu_backend.rs),
already validates revisions and dependencies before batching project data,
optional ephemeral data and provenance. It uses `transact_graph_batches` and
reports a data transaction head. Ephemeral validation can explicitly reference
the project graph. `snapshot`/`--at` combine project and ephemeral state. This
is an actual cross-file compatibility dependency, not a reason to put unrelated
projects in the same writer queue.

SQLite permits only one writer per database. WAL readers can overlap a writer,
but WAL transactions over attached files are atomic per file, not across the
set. A crash can leave only some attached files committed. See SQLite's
[isolation](https://sqlite.org/isolation.html),
[WAL overview](https://sqlite.org/wal.html) and
[ATTACH rules](https://sqlite.org/lang_attach.html). These are engine limits,
not gaps that an outer SQL savepoint over independent connections fixes.

## Architectures considered

| Architecture | Independence and linking | Atomicity and cost | Decision |
|---|---|---|---|
| Several graph queues feeding one existing file | Joins remain cheap; SQLite still has one writer | Existing atomicity retained, write contention remains | Baseline only; does not meet the requirement |
| Independent graph-file actors, composed read views, explicit participant journal | Separate writer and read pool per file; canonical RDF joins | Local transactions stay local; related multi-file batches need durable coordination and visibility rules | Recommend |
| A service/process per graph with result-level federation | Stronger process failure isolation; independent files | Current federation cannot perform general cross-provider joins; adds transport, recovery and authorization surfaces | Defer; a process boundary may later host the same actor protocol |

The recommendation is a storage/router change, not permission to reinterpret
existing read-only attachments as writable or to make arbitrary multi-file
transactions partially successful. Source interfaces and fixtures precede any
client migration.

## Ownership, routing and resources

Introduce a versioned owner-controlled GraphCatalog and a GraphStoreRouter.
A catalog entry binds an exact graph IRI to an opaque file identifier, storage
mode, generation, store identity, log epoch, schema/policy revision and owner
fence. A URI supplied in a request never becomes a filename or an ATTACH alias.
Resolve aliases and registered graph IRIs through the existing authorization
boundary; canonicalize paths and reject symlinks, unexpected identities,
duplicate writable owners and concurrent migration generations.

Each routed graph actor owns one writable Store connection, one durable writer
fence, a bounded FIFO mailbox, commit notifications and its own read pool.
All writes to that graph, including CLI, imports, forks, maintenance and events,
use that owner or refuse. A router cannot create a second writer from a fallback
handle. Startup or ownership uncertainty makes that graph unavailable, not the
whole service writable under weaker rules.

Ordinary commits do not write the catalog, allocate global terms or acquire a
service-wide writer mutex. Catalog changes are infrequent control operations;
routers pin immutable generations. Queue slots, query admission, embedding work,
feed fan-out, caches, checkpoints and cancellation are accounted per graph.
Total workers, connections and memory also have explicit deployment bounds;
per-graph isolation is not unbounded thread/process creation. An exhausted graph
must not borrow every other graph's reserved capacity.

File actors are independently writable on the same host. Shared CPU, disk,
fsync and embedding hardware remain shared resources: removing mutex contention
does not promise physical latency isolation. No production thread budget changes
are authorized here. A supervised process per actor is a later isolation option.

Graph class, overlay parent and owner binding remain explicit. A committed graph
maps to one writable file. Its internal ROOT is a storage detail exposed only as
that registered logical graph, never merged into the request's default ROOT.
Private graph metadata is not an extra public graph. Legacy unrouted graphs stay
in the original file and keep their existing default-dataset behavior.

## Terms, links and query semantics

Preserve IRIs as identity. Terms remain file-local allocations in disjoint term
spaces, reusing the existing space/alias machinery where valid. Allocate a space
at file creation, not on every term insertion. Refuse collisions, exhaustion and
ambiguous restores before mounting. Do not add a hot shared term-registry writer.
Never compare `Value::Ref` blobs or numeric term IDs across files as RDF identity.
Canonicalize IRIs and literal identity before joins, DISTINCT, grouping, ordering,
paths and output; preserve SPARQL bag multiplicity separately from RDF identity.

A query first authorizes and resolves its complete dataset against a pinned
catalog generation. Explicit `GRAPH`/`FROM` route to exact owners. Default ROOT
stays unchanged. `GRAPH ?g` ranges authorized graphs only and has a configured
fan-out limit. Unknown graph keeps SPARQL's match-nothing semantics; a known
selected graph that is unavailable yields an incomplete-read error, never an
empty successful answer. No partial default answers on a failed participant.

The read coordinator builds a versioned composition view using read-only
connections. It applies each file's historical predicate and publication filter
at its selected head, then uses the existing local join evaluator. Current
static `facts_source` SQL and open-time aliases need an explicit cut-aware seam;
they cannot be reused unchanged against dictionaries that keep growing.
An implementation may refresh aliases from monotonic dictionary watermarks or
resolve terms lazily, but it must pin that mapping per query and prove equivalent
results. It must not load all graphs into a writable temporary database.

SQLite has a configured attachment limit; do not assume all files fit one
connection ([limits](https://sqlite.org/limits.html)). The first composed-reader
implementation has a bounded attached set. Over-limit queries refuse before
execution. A later multi-provider join engine needs its own parity and resource
proof before it can expand that bound. Remote `SERVICE` retains its separately
declared consistency and trust contract.

### Local transactions and multi-file cuts

A transaction coordinate is `(store_id, log_epoch, local_tx)`. A graph's ordinary
write receipt includes that coordinate and catalog generation. No scalar is
compared across files. A composition cut names every selected graph and its
coordinate, policy revision and generation; its canonical digest is the cursor.
A clock timestamp is not a global commit sequence.

Two read modes are explicit:

- **Strict snapshot:** capture published heads while holding short participant
  commit fences over the selected dataset and catalog generation. Acquire all
  fences with try-all/release-on-failure and a deadline; never hold one graph
  while waiting in another graph's writer queue. At the successful common
  capture point, all selected heads represent one published state. Release
  fences before evaluation and use retained history at the captured heads.
- **Vector cut:** opt-in historical/replica reads may use an explicitly supplied
  cut that was never one wall-clock instant. Check identity/epoch, retention,
  catalog and causal closure, including batch publication dependencies. Return
  that weaker contract in the result. Missing coordinates or dependencies refuse.

Strict is the compatibility target for ordinary composed reads; it must not
silently fall back to vector mode under contention. A dependency on a hot graph
can make that composed query time out. Local writes on an unselected graph do
not participate in this barrier. Readers cannot accidentally expose partially
installed multi-file batches through a fast path.

Existing scalar `as_of_tx` and Seeds `--at N` keep exact meaning on their legacy
single file. A routed one-file client can explicitly bind a scalar to its known
store/epoch. A scalar spanning files refuses and requests a cut or a preserved
legacy-coordinate mapping. New cut tokens are versioned; do not reinterpret old
numbers. Valid-time filtering remains independent and must compose with the
per-file transaction cut, including retraction valid times. Unsupported temporal
operators refuse rather than reading current data as historical data.

## Write atomicity and crash protocol

Single-graph operations use the existing transaction/guard pipeline and local
receipt in one SQLite transaction. Conditional revisions are checked on that
owner. An idempotency key binds principal, operation digest, graph, generation
and epoch; changed payload or ambiguous outcome never means blind retry.

Keep existing multi-graph writes in the legacy file until the following bounded
participant protocol has passed review and crash qualification. An initial
router must reject any batch spanning writable files before staging facts.
Seeds cannot move to separate project/ephemeral/provenance files at that stage.

For a supported multi-file batch, its deterministic home participant stores the
coordinator journal. Disjoint participant sets need no shared global journal.
A durable batch identity binds the verified principal, all target and validation
dependencies, expected heads, exact payloads and catalog/policy revisions.

1. Acquire all participant write/validation fences in canonical order using
   bounded try-all admission. Recheck revisions, ownership and authorization.
   Prepare each exact delta in an immutable intent table; persist its digest,
   expected head and recovery fence. No application fact is visible yet.
2. After every prepare receipt is durable, fsync the home journal's COMMIT
   decision. Before this decision recovery can abort preparations; after it,
   recovery must finish the same batch, never choose abort or generate new IDs.
3. Apply each prepared intent through the guarded local transaction pipeline,
   once, with a durable `(batch_id, digest, local_tx)` receipt. This includes
   local provenance/events and any exactly-once transaction effects. Rechecks
   cannot change the prepared decision; an unexpected failure fences the
   affected participants for recovery instead of exposing partial success.
4. After all apply receipts are durable, publish the complete receipt vector in
   the home journal. Only this PUBLISHED record is the external commit point.
   Acknowledge once its durability and participant installation are confirmed.
   Release fences; retries return the same complete receipt vector.

A local batch's COMMIT is not its publication. Participant facts installed in
step 3 are hidden behind their previous published heads until step 4. Every
read path, feed, metric, index, search result and hook must honor publication;
current-state SQL or mutable side indexes cannot bypass it. Use versioned or
publication-aware index views, or refuse that endpoint until it is adapted.
Do not emit externally actionable events from an unpublished participant.

Participants persist the home identity and prepared descriptor. On restart,
reconcile the authoritative decision before serving pending state. A published
home descriptor contains all local heads, so recovery can reconstruct visibility
without an impossible simultaneous update of every file's head field. Cache
publication decisions only with identity/epoch validation. If the home is
unavailable, retain the old published read cut where provable and refuse new
writes to involved participants; otherwise refuse the read too. Unrelated graph
actors continue. The journal is a real dependency for participating batches,
not a new dependency of every local commit.

Crash after a prepare, decision, individual apply, publication or lost reply must
produce either the old cut or the complete new cut, never a mixed result. A
missing/changed receipt, damaged intent or ambiguous restore stays fenced.
This is application-level coordinated commit plus publication, not a claim that
WAL supports atomic cross-file SQL. If this protocol proves too expensive or
incomplete, Seeds stays on its legacy file; partial commits are not a fallback.

## Constraints, policies and security

Each graph pins its own loaded shape/rule versions, namespace policy and key-IRI
contract. Ordinary local guards stay inside the local transaction. Immutable
reference/schema packs can satisfy explicitly declared read dependencies without
writing their owner. General cross-graph invariant enforcement is not provided
by copying today's combined-shapes string into every file.

A live cross-graph constraint needs a declared, bounded dependency set and
invalidation rules for writes on either side. Enlist all affected read/write
owners in the same validation fence protocol. Changing a referenced type must
check the dependent graph as well as inserting the reference. Unsupported
unbounded/global constraints refuse the write before preparation. Constraints
requiring every graph are an explicit global bottleneck; never advertise them
as independent local transactions. Async emit-only rules may use causal cuts,
but cannot masquerade as synchronous integrity guarantees.

An IRI link is not a foreign key, an ownership grant or a security tenant.
Named graphs remain dataset boundaries; retain normal authenticated principals,
graph grants, trust floors and explicit reference contexts. Apply read grants
before catalog enumeration, cut capture and joins; recheck write grants at
prepare. Reject unauthorized join participants even if another permitted graph
links to them. Hidden graphs must not leak through counts, errors or global
search. Storage ownership fencing and content authorization remain distinct.

For a key namespace/type routed across files, activation pins one canonical
key-contract fingerprint in owner catalog policy before those actors can mint.
Every actor validates its local shape against that pin. This rare control
operation prevents two first writers from activating different derivations;
it does not require a global allocation write for each entity. Existing allocated
history must be inventoried before an activation/edit can be admitted. A missing
or incompatible fingerprint refuses mint/import verification, not a guessed
local default. New type/namespace activation remains an explicit identity change.

Session/principal bindings, nonce consumption and signed operation scope also
need an explicit placement contract. Existing attestation persists replay state;
copying its table into several files can reopen a replay window. Reuse the
current verifier and retain one authoritative replay owner for each existing
signature domain until a reviewed graph/file-bound scope makes independent
ledgers safe. A shared replay/decision ledger is a genuine serialization point
for those signed operations and must be measured and stated. Do not invent new
cryptography or bypass that ledger to claim multi-writer throughput. Phase 0 must
identify these dependencies; the affected signed route stays legacy/refused until
its authority and replay contract is qualified, even if unsigned local fixture
writes already progress independently.

Key changes require a new identity plus supersession. Freeze allocated key
contracts, including on restored files. Do not mint a replacement IRI because
term IDs or file paths changed. Moving a graph preserves its logical IRI and
requires a fenced catalog generation switch; moving individual entities is an
explicit semantic operation, not an automatic rebalancing feature.

## Identity, distribution, backup and rollback

Each file has its own store_id, log_epoch, transaction log, forks and change
cursor. Restoring/adopting writable data mints the required fresh epoch under
the existing ownership protocol; copying a file does not create a second valid
writer. Forks and changesets name their source graph/file identity. Multi-graph
forks need an explicit base cut and coordinated promotion; until built, refuse
them across files. A source-local scalar fork base never becomes a global base.

Replication is per-file with publication descriptors and dependency coordinates.
A follower cannot publish a multi-file batch until all required participant
frames and the decision are present and verified. Unsupported bundles remain
unpublished. A service-wide cursor is a versioned cut/manifest, not max(tx).

A consistent multi-file backup includes the catalog generation, every file's
identity and selected head, pending intents, home decisions and publication
vector. Use verified per-file backup/snapshot artifacts, then verify one closed
cut over the entire bundle before publishing it as a backup. Sequential copies
of active `.db` files do not prove that cut; independently backing up each file
does not by itself establish cross-file consistency. SQLite's
[backup API](https://sqlite.org/backup.html) provides a per-file snapshot, not
this bundle protocol. Never discard active WAL state or recovery journals.

Existing local-pack/distribution work composes via explicit store/epoch/cut
manifests and ownership receipts. One graph file per pack is an option to
qualify, not an assumption about the current pack format. Packs may contain
other metadata; redistribution cannot infer grants or merge writer identities.
Private preparation, recovery, credential and replay tables are not public
graph payload. A raw file containing them must not be published as a sanitized
graph pack. Export only the authorized logical graph and approved metadata;
private recovery bundles separately require authority for every participant.
No ordering dependency on push delivery or read-standby work is invented here:
their owners need concrete manifest, epoch, freshness and cut contracts when
consuming the new mode. Those integration interfaces must be reviewed together.

Migration is opt-in per graph with a separately authorized operator packet.
Inventory every graph-bearing side table and term-bearing column from schema;
do not copy an incomplete hand-maintained list. Build a fresh destination,
validate semantic facts/history/IDs, replay an admitted delta while legacy stays
authoritative, then fence that graph's old writer, catch up and switch one
catalog generation. Never dual-write without a proven protocol. Keep old data
and explicit legacy-to-new cursor mappings; shared snapshots/history must
remain readable or refuse with a specific migration boundary.

Before cutover rollback is abandoning the new copy. After new writes, rollback
requires a separately rehearsed reverse reconciliation under fences and a new
ownership epoch; flipping the path back to a stale file loses accepted writes.
Keep all receipts, source files and manifests. On uncertainty stop only affected
writers and recover; do not reset ownership, recycle epochs or conceal a split.

## Phased plan and Seeds acceptance

Every phase needs independent positive, negative and failure controls before
its next source change. These are proposed implementation boundaries, not
permission to create runtime stores or launch a migration.

| Phase | Deliverable and owners | Required proof / exit |
|---|---|---|
| 0: contract inventory | Quipu storage lead plus Seeds owner enumerate actual write/read graphs, revision checks, provenance, ephemeral references, shared types and cursor consumers | Pin source revisions and a complete Seeds operation matrix; document unsupported cases; design review approves boundaries |
| 1: local actors | Quipu adds catalog/router behind opt-in configuration; legacy path unchanged | Two temporary files commit while the other's writer is deliberately held; no fallback writers; generation/fence/path/queue refusals; default ROOT parity |
| 2: composed readers | Quipu adds term/cut/publication-aware read view, strict capture and explicit vector mode | Same/different IRI-ID adversaries; RDF literal identity, bags, paths, aggregates, dataset/grant isolation, hot-graph deadline, unavailable member and attachment-limit controls; old scalar refusal |
| 3: participant commit | Quipu implements intent/decision/apply/publication protocol and bounded constraint dependencies | Inject crash at every durable edge and reply boundary; recover old-or-all-new, idempotent receipt, no early event/index leak; missing home/changed digest/epoch keep participants fenced |
| 4: Seeds source pilot | Seeds adapts backend and remote client to graph routing, batch receipts and cut cursors; Quipu provides compatibility mapping | Run the matrix below on disposable files with primary/replica and legacy comparisons; same IDs, revisions, readiness decisions and history; no weakened batch/provenance semantics |
| 5: delivery rehearsal | Storage/operator, Seeds and distribution/standby owners review exact pins, bundle restore, runtime limits and inverse | Private rehearsal at exact source/config pins; complete CI, one independent source/design review, restore and rollback receipts; explicit operator admission before any real graph moves |
| 6: scoped rollout | Named graph owner executes approved packet and observes natural consumer behavior | Verify serving identities/epochs/cuts and one admitted smoke; monitor per-graph queues and cross-graph errors; keep installed acceptance open for deferred controls |

The first tenant is a real Seeds project plus its ephemeral and provenance
graphs, with a separate linked reference graph and a second unrelated hot graph.
It is not a toy board with every operation reduced to one file.

| Seeds operation | Positive control | Negative/crash control |
|---|---|---|
| Create/edit/close plus provenance | One acknowledged batch has matching data, revision and attribution receipt | Reject stale revision; kill after each participant applies; no data without provenance and no successful partial reply |
| Ephemeral item references shared Action | Exact configured project supplies permitted type at the captured cut | Unrelated graph cannot supply type; shared-to-ephemeral edge rule and missing dependency still refuse |
| Dependency/readiness query | Project/ephemeral/reference join matches legacy result at one strict cut | Concurrent referenced-type/status change cannot produce an inadmissible scheduling decision; unsupported cross-file conditional admission refuses |
| Comments and item location | Comment stays with parent; same stable IDs and revision rules | Shared/ephemeral move still refuses, including a retry after an uncertain response |
| Snapshot, `--at`, export and hydrate | Exact supported local cursor or multi-file cut reproduces IDs/statuses and intended graph scopes | Foreign epoch, partial cut, lost provenance, accidental ephemeral export and unsupported legacy scalar refuse |
| Retained retry/consumer replay | Identical intent returns original complete receipt and cursor | Same key/different bytes, missing journal and restarted stale owner refuse; events do not double-deliver as new writes |

Scheduling/claiming is a write-side decision, not merely a consistent SELECT.
If it relies on mutable state in several files, its preconditions and receipt
must share the participant protocol. If the current Seeds mode cannot express
that contract, leave it on the legacy path; a successful read is not admission.

## Measurement targets and review gate

Use only synthetic fixtures during design/source qualification. Record idle and
held-writer baselines, per-graph queue wait, commit/read p50/p95/p99, throughput,
CPU, fsync, memory, connection count, WAL growth, checkpoint delay and recovery
time. Separate mutex independence from shared-hardware contention.

The deterministic gate is stronger than a throughput chart: while graph A's
writer is held behind a test latch, graph B must commit and serve a point read
before A is released. Repeat with A's queue, embedder and checkpoint held; prove
B never enters A's mutex or the catalog writer. Add overlapping-participant
controls showing that related batches correctly wait/refuse. Prove bounded
queue/fan-out exhaustion and cancellation release every acquired fence.

Proposed performance budget for later review: at the same bounded synthetic
workload, unrelated-graph point-read and commit p95 no more than 1.25 times its
idle baseline, with no unbounded queue growth. Report the raw baseline and
confidence/variance; passing that target does not replace the latch proof.
Numbers are targets, not measured results or a production load-test allowance.

The design exits only with reviewed alternatives, source-linked boundaries,
Seeds pilot matrix, crash/publication invariants, semantic refusal cases and a
phased migration/rollback packet outline. Normal source CI/privacy checks and
one independent nonauthor Codex design review precede implementation items.
Unresolved runtime allocation, schema ownership or client compatibility blocks
the relevant phase, not unrelated design analysis or a different graph's writer.
