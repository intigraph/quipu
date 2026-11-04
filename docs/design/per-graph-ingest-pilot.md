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

## Composed reads are part of the first slice

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

## Qualification sequence

1. Complete the producer/consumer matrix and namespace integration. Land the
   namespace write-path changes first, or refresh this source onto their accepted
   content. Serialize write-path integration rather than stacking incompatible
   server changes.
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
