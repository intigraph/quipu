# Per-graph ingest pilot

Status: proposed implementation contract; no production migration authority.

This is the first application of [per-graph writable stores](per-graph-writable-stores.md).
It isolates regenerable deterministic ingest before moving an application with
cross-graph atomic writes. A successful source fixture does not authorize a
production graph move, a new credential or a new scheduled runner.

## Inventory before routing

Inventory the executing producers, not only their source checkout. Pin executable
and configuration revisions, request envelopes, snapshot keys, graph targets,
markers, authority and all consumers. A producer snapshot key identifies ownership
within a graph; it does not identify a named graph or a separate database.

The initial inventory has three producer classes:

| Producer | Present write contract | Required adaptation |
|---|---|---|
| Code projection | `/knot`, stable repository/file snapshot replacement keys | Explicit configured named graph; preserve entity IRIs, snapshot identity and deletion behavior |
| Commit links | `/knot`, deterministic commit entities and code/work-item references | Explicit configured named graph; preserve author valid time, source and reference IRIs |
| Document mentions | `/knot`, deterministic document links to existing entities | Explicit configured named graph; vocabulary reads retain their authorized reference dataset |

The observed producers omit `graph`, so their destination is currently ROOT.
There is no existing graph-file routing switch to flip. Enumerate actual records
and snapshot ownership before deciding which records move; an IRI prefix alone
cannot distinguish facts asserted by different producers about the same entity.
Unrelated lifecycle ingress remains outside the pilot unless separately inventoried.

Graph destinations come from reviewed configuration or explicit requests, never
deployment-specific constants in `src/`. A graph target never becomes an entity
namespace. Preserve the existing `base_ns` and `code_base` pins. Governance terms
continue to use the governed base namespace. A graph move must not remint code,
commit, document, policy or work-item identities.

## First supported write path

The first router admits only a single explicit committed graph per operation.
Each registered route pins its file, store identity, epoch, graph IRI, generation,
owner fence and policy identity. Reuse the ordinary guarded `/knot` pipeline,
including vocabulary, SHACL, authority, snapshot replacement, refusal audit,
provenance and deferred embedding. A second connection with weaker defaults is
not an equivalent writer. All deferred work and wake notifications stay with the
selected owner.

An operation that needs another graph for synchronous validation remains on the
legacy path or refuses before staging. In particular, preserving ROOT-provided
SHACL context requires an explicit dependency contract; silently validating only
the new file is forbidden. Signed operations retain their existing authoritative
replay owner until a separately reviewed graph-bound contract exists.

All other mutation paths must either use the same owner or refuse that routed
graph before changing any side table. This includes graph maintenance, forks,
imports, snapshot retirement, CLI access, events and direct transaction batches.
The catalog must not advertise an owner before those bypasses are fenced.
Multi-file batches refuse before the first staged fact. Seeds stays legacy.

Bound per-owner queues and reader pools, and the aggregate memory, connection,
attachment and worker budgets. An unavailable owner returns an explicit error;
it cannot fall back to a writable legacy handle or borrow another owner's entire
resource reservation. The graph catalog is not written on ordinary commits.

### Legacy bypass fence

`Store::fence_routed_graph` makes the legacy file itself refuse every write
that would change a routed graph, whichever path or process issues it. It adds
a `routed_graph_fence` row and BEFORE triggers on each graph-bearing column.
The columns are discovered from the live schema; each needs a reviewed class:

| Class | Tables | Fenced operations |
|---|---|---|
| Authoritative | `facts`, `graphs`, `forks`, `writer_graph_instances`, `frozen_packs`, `pendant_repo_graphs` | insert, update, delete |
| Log, staging or cursor | `graph_events`, `graph_consumers`, `snapshot_uploads`, `denial_quarantine` | insert, update (retention may delete) |
| Reference | `dataset_members` | none: consumer dataset configuration must name routed graphs |
| Derived | `substring_facts`, `lexical_fts`, `qp_proj_*` | none: rebuilt from fenced `facts` |

An unclassified graph-bearing column refuses installation and
`verify_routed_fence`; so does a missing trigger. Term-id columns resolve the
graph IRI at write time, so a graph interned after fencing is still covered.
The retained legacy copy stays readable and cannot be deleted through the
fence, which keeps it available for reverse reconciliation.

Consequences to plan for, not hidden: legacy compaction or retention that
deletes authoritative rows of a fenced graph aborts; a denial-quarantine row
for a fenced graph aborts, so the router must refuse such requests before the
legacy gate; installed triggers add a per-row predicate on legacy writes whose
cost has not been measured yet. Tables created later, including on-demand
derivations, are covered only after `verify_routed_fence` passes or the fence
is re-installed. The fence is never installed in an owner file. Removing a
fence row after the owner accepted writes still needs fenced reverse
reconciliation.

The lexical profile is qualified both when its FTS schema predates the fence
and when it is initialized afterward. `lexical_fts.graph_id` is a derived
column from `lexical_source`, not an independent graph writer. No fence trigger
is installed on the virtual table. Routed fact writes still refuse, unrelated
fact writes still maintain the index, and a drop/backfill rebuild preserves the
retained index without releasing the authoritative fact fence.

### Producer source cutover

The observed producers write ROOT and own their facts by the transaction
source tag `snapshot:<key>`. Moving them is therefore not only a named-graph
fence: an old or misconfigured producer that still omits `graph` would keep
writing ROOT copies, and existing ROOT copies would stay current for ROOT
consumers although their producer can no longer replace them.
`Store::cut_over_routed_source(tag, graph, revision, generation, ts)` requires
the graph fence first. Then, in one savepoint, it retracts that tag's current
ROOT facts (bitemporally, so history stays readable) and records the tag. A
`transactions.source` trigger then refuses any new transaction under it. No
producer write can land between the retraction and the fence. Identical
re-runs retract nothing; another route for the same tag refuses.
`verify_routed_fence` also proves the source triggers exist. Which tags move
comes from the executing-producer inventory, never an IRI prefix.

### Slice copy and source parity

`Store::export_routed_slice(tag, graph)` returns a producer's current facts
in canonical form (IRIs resolved, never file-local ids). It uses the same
source-retraction plan that `cut_over_routed_source` retracts, so the copied
set and the retracted set cannot drift. `Owner::import_slice` asserts it into
the owner graph through the ownership seam, under the same tag and with its
original valid time. A second import refuses. The qualified sequence is:
take an immutable baseline, export, import, fence the graph, cut the source
over. Then compare the canonical fact multiset and the SPARQL result rows in
the owner against the baseline. Controls: moved facts left ROOT, an unmoved
producer stayed in ROOT and never reached the owner, and the producer's next
`replace_snapshot` in the owner replaces the copied facts.

A consumer parity test runs one consumer query through the strict composed
reader before and after the move. The query selects ROOT as the default graph
and the routed graph by `FROM NAMED`, as `{ ?s ?p ?o } UNION { GRAPH <g> {...} }`.
It requires identical W3C bindings, including datatypes and language tags.
Controls: after the move, ROOT alone and the routed graph alone each give only
their part. This shows the consumer-side rewrite a migrated consumer needs.
Existing ROOT-only queries do not see moved facts until they adopt it.

Still not covered: history copy, paging beyond the endpoint row cap, the
production stored-query inventory, `/search` and code-intelligence joins.
Those remain required before any cutover.

### Write router

`graph_router::Router::admit` takes the catalog, the legacy writer's store and
exactly one prepared owner per route. It refuses unless the legacy fence names
exactly the catalog's routes at the catalog revision and each route's
generation, and `verify_routed_fence` passes; until then no route is
advertised. `Router::knot` dispatches by `Catalog::write_target`: a routed
graph runs only in its owner, any other target runs the ordinary pipeline on
the legacy writer, and the request and input must name the same graph. A
caller that bypasses the router still meets the legacy file's fence.
`Router::recheck` repeats the fence check, e.g. after legacy maintenance. The
router is not wired into the server, CLI or startup.

### Default-off source pilot

`graph_router::IngestPilot` defaults to disabled. Its library admission method
returns no router without invoking owner preparation. When explicitly enabled,
it accepts exactly one selected catalog graph and verifies the legacy fence
before preparing that owner, then repeats ordinary router admission. The caller
must establish that the selected producer is regenerable; the option does not
infer ownership or supply a production configuration switch.

`Router::routed_knot` takes no legacy writer reference. A routed request executes
in its prepared owner; a legacy selection returns `None` without writing, so an
adapter can acquire its legacy mutex only after selection. A refusal stays an
error and must never trigger legacy fallback. `Router::knot` retains the existing
combined interface using this dispatch method. Authentication and provenance
remain the caller's scopes, and ordinary owner guards still apply.

Disposable qualification holds both the legacy application mutex and a SQLite
`BEGIN IMMEDIATE` transaction while a worker replaces an ingest snapshot and
reads its current value. Completion is checked before releasing either lock.
Idle and held-writer samples report microseconds and p50/p95, with unchanged
legacy `total_changes` and `data_version` witnesses. These are synthetic source
controls, not natural-run acceptance or a production memory profile. Enrolled
startup, server dispatch, composed consumer parity and migration/inverse
qualification remain required before a production route is advertised.

### Owner admission and durable completion source boundary

The prepared server boundary consumes one admitted single-graph router and
reserves its configured writer queue and existing reader pool. It installs no
HTTP route, startup resolver or runtime switch. Every owner must have exactly one
shared boundary; constructing another queue for the same owner is not supported.
The owner must have been prepared against the identical immutable catalog,
including its resource budget, rather than an independently reconstructed route.

Routed writes use an owned per-owner semaphore through the ordinary tracked
blocking helper. They never acquire the legacy global write permit. Active and
waiting requests have a bounded combined population. Queued cancellation releases
only that reservation; after dispatch, the actual worker retains its reservation
and permit through effect, deferred embedding and receipt finalization even if
the request future disappears. Request deadlines, identity, provenance and write
kind still cross the same blocking boundary. Status reads reserve an existing
owner reader before dispatch, refuse when busy, and never queue behind a writer.

The first durable contract supports stable authenticated, unsigned, unscoped
single-graph requests. Exact graph, catalog, generation and policy pins must
match before admission. Scoped v2 refuses before consuming an ID: its existing
outer writer-scope transaction does not compose with the current outermost
ticketed effect contract. Ordinary scoped v1 keeps its writer-scope guards;
it does not gain a durable transport contract from this boundary.

Admission, atomic commit evidence and terminal completion all live in the owner
file. The original wire write ID remains the ledger primary key. An owner-context
digest binds catalog revision, graph, file/store identity, epoch, generation and
policy in the same admission row. Changing context cannot turn a consumed ID
into a fresh execution. Body, principal or context mismatch produces UNKNOWN or
refusal, never permission to replay. A nullable private column admits old legacy
rows without re-keying them; status reads never migrate the ledger. Legacy status
does not disclose an owner-bound receipt. Explicit sealing retains the consumed
ID and cannot seal an already committed effect.

These source primitives do not reconcile historical IDs between different files.
Physical cutover still needs a reviewed disposition for legacy receipt history,
ambiguous attempts and downgrade prevention across file boundaries. Startup S1
admission, graph grants, dependency datasets, bypass fencing and composed reads
remain necessary before a server can advertise this boundary.

## Composed reads are part of the first slice

### Explicit enrolled factory source checkpoint

The native `IngestPilot::admit_enrolled` factory accepts one explicit
`EnrolledPilotConfig` and a lazy credential callback from a trusted initializer.
The disabled pilot does not inspect that configuration or invoke either callback.
An enabled pilot first verifies the exact single-graph catalog and legacy fence,
then validates the named HTTPS witness profile and independently supplied served
policy. Only then can it resolve credentials and call the existing S1 owner
constructor. It attaches to an existing witness lineage; it cannot seed genesis,
enroll a file, install a legacy fence or fall back to an ordinary writable open.
Witness reads are live and bounded per request. The inventory's profile and store
identity must match the selected source exactly; mismatches remain UNKNOWN.

The configuration includes a precomputed aggregate memory allocation. Passing
that allocation is not evidence that the initializer counted every connection.
Credential file protection, secret retrieval, the aggregate reservation and
installation of the complete served writer configuration remain the trusted
initializer's responsibilities. The factory does not discover environment
variables or choose a bucket, principal or inventory implicitly.

This checkpoint adds no TOML switch, HTTP route or production startup caller.
It must be compiled and qualified with the existing S1 object-store emulator,
enrollment rejection controls and owner/router suites before use. Refusal before
credentials, disabled-mode inertness and policy/profile mismatch controls are
included as source tests; their execution is a separate bounded build gate.
The subsequent server initializer must retain exactly one shared prepared owner
and refuse publication until composed-read and mutation-path coverage qualify.

Ingest previously visible in ROOT must remain visible to its existing consumers
through explicit, reviewed dataset configuration. This is a consumer migration,
not permission to make every registered graph part of ROOT by default. Unrelated
legacy ROOT queries retain their dataset and authorization behavior.

Before enabling a route, compare the actual stored-query inventory, `/search`,
code intelligence joins and application reads against one immutable baseline.
Record result multiplicities and canonical RDF terms, not only total row counts.
Page beyond the endpoint row cap; a truncated matching prefix is not parity.
Keep positive controls for moved facts, unchanged legacy facts and excluded graphs.

The composed reader must see terms created after a connection was opened.
Existing attachment aliases are constructed at open and are not a live dictionary
contract. Qualify a cut-pinned refresh or equivalent resolver with new shared IRIs,
distinct file-local IDs for one IRI, same numeric IDs for different IRIs, literals,
bags, joins, paths and aggregates. Do not compare raw `Value::Ref` IDs across files.

Use the strict common published cut from the parent design. Capture selected heads
under bounded participant fences and release them before evaluation. A selected
unavailable graph or attachment-limit violation refuses the complete read. A
scalar transaction ID spanning files refuses; never reinterpret it as a global
head. Search, counts and feeds must obey the same visibility and graph grants.

The private native prototype can pin ROOT separately with
`Catalog::with_legacy_participant`. Its exact store identity, epoch, policy,
generation and opaque placement remain distinct from routed graph entries.
`Owner::prepare_with_memory` can then consume that configured legacy writer and
open its reserved readers. A strict selection may include this participant in
addition to the routed-owner bound. The shared allocation must reserve its writer,
ownership-fence reader and application readers before scratch admission succeeds.
ROOT remains absent from `Catalog::routes` and is never a routed write target.
Queries retain the existing ROOT syntax: `FROM <urn:quipu:graph:root>` selects
ROOT as the default graph, and `FROM NAMED <graph>` selects a routed join input.
`GRAPH <urn:quipu:graph:root>` remains invalid.

Composed authorization must explicitly include ROOT to select it. The scratch
copy maps only physical graph zero to ROOT; other named graphs in the legacy
file are excluded. The legacy participant uses the same nonwaiting commit fence,
retained SQLite snapshot, identity checks and deadline as routed participants.
This primitive does not integrate the production legacy mutation entrypoints:
all of those must use this same fence before a server may advertise composed
strict reads. Evaluator/RSS reservation and durable startup/bypass qualification
also remain separate requirements.

The shared admission initializer also requires an explicit
`ComposedReadReservation`: runtime headroom, each view's scratch/dictionary/model
share, each outstanding query's evaluator/output share, and a serialized-body
ceiling. These shares come from the evaluator half of the same aggregate service
budget as the SQLite allocation. Zero, overflow and overcommit refuse before
capture. There is no production sizing default; native controls use synthetic
reservation values to exercise admission, not to certify a production profile.

A view holds its view lease through destruction of scratch state. Each query
acquires a separate lease before parsing/evaluation. The returned
`ComposedQueryOutput` retains that lease with its bytes, even after the view is
dropped, so retained responses cannot silently free capacity for another query.
The W3C output writer refuses before extending its buffer past the declared
serialized-body ceiling; overflow returns an error rather than a partial answer.
Consumers borrow bytes and the serializer's media type. Any consumer-made copies
need separate accounting. Existing fresh policy/cut recheck requirements still
apply immediately before publication; retaining bytes is not publication authority.

These are reservation and output-byte controls, not an allocator meter or proof
of a hard evaluator RSS bound. Parser, resident-model, join, format-conversion and
runtime peaks still require workload measurements and the aggregate process/cgroup
boundary. The in-process prototype must not be advertised as an RSS-isolated server.

The private native owner adapter reuses the handoff implementation's
[`ownership_operation`](ownership-commit-scope.md) for both its ordinary knot
pipeline and its later deferred-embedding write. Inference holds no lineage
latch; the second write reacquires and rechecks the same authority. A retirement
between those phases refuses the vector write while preserving the earlier
committed facts and existing post-commit error semantics. It does not retry facts
or pretend that a late embedding failure rolled them back.

This integration adds no startup admission or generation-pin setter. Private
controls use the handoff implementation's existing test-only pin. The actual
protected startup adapter, external admission and producer routing remain separate
requirements; a simulated ACTIVE journal cannot enable a production owner.

## Qualification sequence

1. Complete the producer/consumer matrix and namespace integration. Land the
   namespace write-path changes first, or refresh this source onto their accepted
   content. Serialize write-path integration rather than stacking incompatible
   server changes.
   Router/write-path source lands after the namespace S3a–c changes (#1008,
   #1011, #1014), or rebases onto them; allow one Quipu write-path PR at a time.
2. In disposable files, hold owner A's writer behind a latch. Owner B must commit
   and answer a point read before A is released. Repeat for queue, embedding and
   checkpoint pressure. Assert no admission into A or a catalog writer.
3. Exercise snapshot replacement and every local guard through the real producer
   envelope. Refuse foreign ownership, stale generation, path substitution,
   policy mismatch, unsupported multi-file validation and signed replay domains.
   Prove refusal leaves application facts and graph-bearing side tables unchanged.
4. Compare composed reads at a common cut, including post-open dictionary growth,
   authorization exclusions, unavailable selected owner, fan-out limits and
   transaction-coordinate refusals. Run negative controls that would fail if
   routing were ignored or only the legacy file were queried.
5. Rehearse export, restore, delta catch-up, cutover and reverse reconciliation at
   pinned revisions. Only then prepare the scoped operator packet.

## Cutover and inverse

Build a fresh destination from an authorized graph slice. Verify the complete
schema-derived fact/history/side-table inventory, stable entity identities,
snapshot ownership, cursor mapping, policies and semantic query results. Do not
copy a live `.db` file without its snapshot/WAL contract. Keep one working copy
at a time unless capacity for additional copies is measured and reserved.

Keep the legacy owner authoritative during preparation. Fence the selected old
writers, catch up the admitted delta and atomically publish one catalog generation.
Never dual-write. Retain the source data, manifests, epochs and all receipts.
After accepted writes in the new file, rollback requires fenced reverse
reconciliation and an appropriate new ownership epoch; routing to a stale old
copy would lose accepted data. Rehearse that inverse before cutover.

Replace the clock trigger with a Chaski scheduled reaction in the same cutover
packet. Preserve lane ordering, non-overlap lock, persistent incremental markers,
indeterminate-write custody, outcome receipts and liveness/deadline monitoring.
Do not let cron and the reaction race on separate locks or state directories.
Verify one real scheduled reaction, its signed execution and retained outcome,
then verify the old cron entry is absent through the supported locked editor.
Rollback restores exactly one trigger and the reconciled authoritative route.

Compare pinned natural-run windows before and after. Separate main-store ingest
writer wait/hold from total service wait and shared CPU/disk contention. Acceptance
requires near-zero main-store ingest writer time, independent-owner progress,
unchanged qualified query results and a successful scheduled reaction. Source
merge, process health or a lower global counter alone does not close the pilot.

## TLS and server adapter source checkpoint

The enrolled factory accepts an optional, explicit DER trust anchor (bounded
at 64 KiB); absence retains ordinary public roots. It never disables certificate
or endpoint-name verification. A disposable conditional-object emulator behind
a real TLS bridge qualifies both untrusted-certificate refusal and successful
S1 admission with the selected anchor, followed by a routed owner write.
The fixture generates its own short-lived certificate and key with OpenSSL;
it uses no deployment credentials or witness service.

The server's `PreparedOwner::initialize` converts the admitted factory router
into one shared owner admission boundary. OFF invokes neither credential nor
writer-configuration callbacks. Its source HTTP adapter shares that same Arc
across `/knot`, `/knot/status` and `/knot/seal`, requiring v2 completion plus
exact catalog revision, owner generation and policy headers. Missing, duplicate
or stale pins refuse through the owner boundary; errors never select legacy.
The mounting initializer must retain the ordinary authentication middleware.

This adapter is intentionally not mounted by production main. There is no
TOML activation switch, listener, enrollment, migration or automatic catalog
publication in this checkpoint. Composed-read coverage, complete mutation-path
fencing and a combined factory-to-HTTP startup qualification still gate the
production mount. The factory TLS and server HTTP fixtures are distinct
controls; passing them does not claim a live cross-store read contract.

### Combined startup and strict dataset source checkpoint

The source-only `PreparedDataset` initializer consumes the configured legacy
writer and admits the enrolled pilot through the same verified TLS/S1 factory.
Disabled startup returns the original writer without resolving credentials or
configuration. Enabled startup requires an explicit legacy participant in the
same catalog and a memory allocation covering both writer/fence/read pools.
Its HTTP adapter retains the same owner Arc used by strict dataset captures.

The internal read callback accepts only explicit ROOT and/or selected-owner
participants. It uses `Owner::read_strict` to retain both SQLite snapshots,
identity/epoch pins and transaction coordinates through evaluation. Unknown,
duplicate, empty, over-bound or expired selections refuse without evaluation;
there is no partial answer or writer fallback. Callers still own authentication,
dataset authorization and evaluator budgets. This is not a new HTTP query route
or a general cross-file SPARQL evaluator. Per-participant SPARQL projections in
the combined fixture check visibility in the captured stores; arbitrary joins,
RDFS/temporal/default-dataset parity remain separate publication prerequisites.
Production main still does not mount the owner adapter or initialize this path.

### Authorized composed HTTP read transport

`PreparedDataset::routes_with_reads` adds a bounded JSON `/query` handler to the
same admitted owner routes. The trusted initializer supplies an existing read
policy, named-credential verifier registry, deadline and the same aggregate
composed-read admission used to prepare the stores. Requests select one or two
explicit participants; selection is not a grant. Empty, duplicate, excluded and
unavailable participants never select a legacy query fallback.

The handler verifies the presented named credential against both current
registry evidence and the identity established by outer authentication. It
authorizes the complete dataset, captures the strict cut, evaluates through the
existing composed evaluator and rechecks fresh policy and credential evidence
before returning buffered output. The HTTP body owns the original output memory
lease through its last consumer; constructing a response does not release that
reservation or copy the response into an unaccounted buffer. Exhaustion and
unverifiable captures return an unavailable response, never a partial result.

The enrolled TLS/S1 HTTP control proves a positive ROOT/owner join, missing and
revoked credential refusal, grant revocation and excluded-scope refusal. It also
holds the response body across a second request to prove that resource admission
stays bounded until the consumer drops the body. CI requires this named test to
execute. This transport does not initialize grants, configure production startup,
enroll or move files, or enable the pilot. Main wiring, full mutation/consumer
qualification and the scheduled physical cutover remain separate acceptance.

### Enrolled evaluator qualification control

The source control joins ROOT and the owner graph through the actual
authorized composed evaluator after real TLS/S1 startup and the admitted HTTP
write. Both pools open before the new owner terms are written. One explicit
service allocation covers the legacy and owner handles plus a scratch view;
the view retains the shared admission permit through evaluation. The query must
return the shared IRI and its language-tagged value, while an excluded graph
refuses. Publication rechecks the captured bindings and read authority.

The focused startup group passes all five tests, including this join; the
composed authorization group passes 14 tests. Full witness-feature Clippy and
all `just check` gates pass. These are source qualification controls. They do
not certify general evaluator parity, RSS sizing, production mutation fencing
or physical cutover. No production query route is added. Startup remains OFF.
