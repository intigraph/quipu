# Single-writer three-way admission

Status: source preparation. No server write path uses this proposal. The
`write_base` module only binds coordinates; it neither validates a slice digest
nor acquires a writer reservation. Separate scoped runs passed 13 coordinate
controls, five physical-reader controls, and two prepared-batch controls.
Full publication gates and integration acceptance remain open.

## Current behavior and the missing boundary

`store/set.rs::set_triple` reads a complete current slot and passes its values to
`store/conditional.rs::check_expected`. The latter reserves SQLite's writer
before comparing that slot. This protects against a writer racing the internal
read. It cannot recover the state a client read before issuing its replacement.
An earlier client read therefore needs a declared base, not another current-slot
compare. Existing conditional-write contracts must remain intact during rollout.

## Base declaration and policy

Reuse `resolver_contract::SourceBinding`: store identity, log epoch, graph,
base transaction, exact touched scope and base slice hash. Numeric transaction
ids alone are not portable across stores or restored epochs. The writer captures
its identity, head and oldest complete retained transaction while holding the
existing reservation. History completeness is an explicit store guarantee;
`MIN(facts.tx)` is not a retention watermark.

`write_base::resolve_write_base` checks the coordinates and retained range.
It preserves the declared base even when newer transactions exist. A supplied
but unavailable or mismatched base always refuses; it never becomes an unbased
write. The caller must still read and verify the exact touched slice and input
hashes. `scope: None` and `scope: Some([])` keep their existing distinct meanings.
This writer path requires an explicit touched scope; it must not turn `None`
into a full graph scan.

Legacy policy is explicit per graph: `RequireDeclared` refuses a missing base;
`UseCurrentAndFlag` resolves to the reserved head and reports `legacy_unbased`.
There is no default in the source helper and no configuration activation in
this slice. A counter or response flag is not permission to select compatibility
mode. The operator chooses the rollout policy separately.

## Reservation, reads and planning

1. Validate request size and scope before acquiring the writer.
2. Acquire the existing writer reservation before reading head, policy, shapes
   or any current slot. Preserve normal denial/verdict transaction behavior.
3. Bind the declaration, read only the declared `(graph, subject, predicate)`
   slots as of base, and independently verify the slice hash. Do not call
   `facts_as_of_in_graph` and filter its full-graph result afterward.
4. Historical membership must account for assertion `tx` and `retracted_tx`.
   A later retraction must not erase the older base view. Received `[t,t)` rows
   are evidence, never live slot values. Distinguish transaction state from a
   valid-time query; a `tx <= base` filter alone is insufficient.
5. Lower request operations into the incoming complete slot relative to base.
   An assert batch is a delta, not implicitly a replacement. Preserve explicit
   deletions and authored times; do not manufacture deletion times from base tx.
6. Feed these inputs to the shared `merge_core::plan_merge`, with shape-derived
   cardinality. Do not duplicate its per-value rules. Inference, list lowering
   and validator/profile identity remain common-core responsibilities.
7. Persist the common receive/decide sequence and existing authenticated master
   receipt under this reservation. Recheck bound validator inputs before commit.

## Pending conflicts and dependency

The current planner suppresses both operation vectors when any slot is held.
It cannot yet persist an incoming conflict receipt plus a pending decision.
Do not bypass this by independently writing a receipt or overwriting the slot.
The shared decision schema, receipt ordering and pending/resume implementation
must supply this behavior first. Reuse its decision identifiers and show/resolve
surface; do not introduce a second writer-specific decision store.

A real conflict returns a pending result only after its receipt and decision
record have committed together. Current slot values stay unchanged. A refused
receipt transaction returns refusal, not pending. Resume captures a fresh head
under a new reservation and verifies the bound inputs; changed state invalidates
the old plan. A transaction id in a response proves committed receipt history,
not that the proposed value became current.

## Qualification before wiring

The initial pure tests cover declared vs current base, explicit legacy flags,
strict graph policy, foreign store/epoch/graph, future/expired history and
malformed capture. They do not prove Store reads, authentication or locking.
Store integration must additionally demonstrate:

- stale single-value replacement becomes pending without overwriting current;
- compatible set changes merge and equal changes flag convergence;
- later retracts reconstruct the earlier base; empty receipts never enter it;
- a second independent writer is excluded until commit or rollback;
- pending receipt and decision persistence roll back together on denial;
- retry and resume remain idempotent and revalidate the current head;
- graph isolation and existing CAS clients retain their contracts;
- bounded touched-slot reads and the existing query/load budgets hold at scale.

Compilation, focused tests, reference/source witnesses and performance checks
remain required. This source slice claims none of those results and activates
no endpoint, graph policy, writer or database migration.

## Explicit slot preparation and future closes

`prepare_scoped_write_base` adds an explicit request slot budget and exact
set equality between touched slots and declared scope. It refuses whole-graph
scope, duplicate/empty slot keys, zero budgets and over-budget requests. An
explicit empty scope stays empty. Legacy compatibility also requires explicit
bounded touched slots. Terms must already be canonicalized by the adapter;
this helper grants no syntax, hash, history or write authority. No production
caller is wired. The two added scope tests are source-only and unrun.

C7b in `merge-behaviour.md` separates transaction state from valid-time reads.
A future close at `Tz` must remain visible at `Td < Tz` and disappear exactly
at `Tz`, retaining `[T0,Tz)` and one receive close at `Tz`. Neither the base
transaction number nor admission time may replace that authored close time.
Touched-slot base reconstruction therefore needs both its transaction cutoff
and an explicit valid-time coordinate; it must not use wall-clock CUR to judge
the fixed-date case. A current-value cache alone cannot represent the scheduled
close. Future integration tests must observe both actual as-of reads, the
history interval and receive transaction independently. Correct history plus
an invented current projection is insufficient. Early close at Td, visibility
at Tz, missing as-of reads and changed authored times must defeat qualification.
This source slice adds no database reader or temporal mutation.

## Prepared read cuts (historical source checkpoint)

`prepare_slot_reads` copies the explicitly bounded touched scope and fixes two
Store `AsOf` coordinates: declared base tx and captured reserved head tx, at
one caller-supplied canonical UTC valid time. No `now` default is available.
It refuses transaction IDs outside Store's signed range. Accessors borrow the
private captured data, so later mutation of the request slots cannot widen it.
The coordinates remain declarations, not evidence that a reservation is held.
Two new tests cover independent tx cuts, Td/Tz preservation, copied scope,
implicit/noncanonical time refusal and unrepresentable transaction IDs; these
new tests are unrun. The previous two scope tests passed in the admitted batch.

The existing `store/reads.rs::facts_as_of_in_graph` combined tx/time branch uses
the stored `valid_to` directly. In contrast,
`sparql/triple/temporal_visibility.rs::add_conditions` allows a closure recorded
after the pinned transaction to retain the earlier open interval. Do not wire
the prepared cuts to the full-graph helper and assume either bounded work or
historical interval reconstruction. A future touched-slot reader must qualify
this difference with a row closed after the base, an originally finite interval,
a future authored close at Tz, and both Td and Tz as-of observations. No existing
read semantics are changed in this source checkpoint.

The prepared object also copies the validated `WriterSnapshot`: store identity,
log epoch, graph, oldest-complete retention watermark and reserved head. This
matters especially for explicit legacy mode, whose resolved base has no declared
`SourceBinding`. The adapter must not reconstruct that identity from a later
mutable request or current global store. `writer_snapshot()` provides immutable
caller evidence, not an attestation that the reservation is held. A new source
test changes all five original snapshot fields after legacy preparation and
requires the captured identity and both tx cuts to remain unchanged; it awaits
the queued Cargo qualification.

## Bounded physical slot reader (source-stage)

`Store::bounded_slot_facts_at` is the next reader seam, with no production
caller. It requires both captured transaction and canonical whole-second UTC
valid-time cuts. The graph/entity/predicate prefix of `idx_geav` bounds the
physical relation before any history filtering. At most `max_claims + 1`
physical rows are inspected; exceeding the cap refuses the entire read,
including when excess rows are future, duplicate or invisible. Encoded value
and interval bytes are counted before owned allocation/decoding. Caps are
limited to 4096 claims and 4 MiB; they are not a time or reservation guarantee.

A retraction recorded after the pinned transaction restores the assertion's
original open end, as in the SPARQL temporal visibility rule. An originally
finite interval keeps its bound. An empty received interval is never live
at or after its transaction. Other timestamp spellings, including fractional
precision, explicitly refuse in this source slice; no precision is discarded
to make an input fit. A general timestamp contract remains an integration gate.
Returned facts represent distinct RDF values, not all provenance claims.

Five Store controls cover later-close/base reconstruction and Td/Tz,
original finite and empty intervals, graph/slot isolation, physical-row and
byte overflow, and explicit cuts/unsupported precision. These passed in the
scoped run at `82e48796`. This does not change `facts_as_of_in_graph` or wire the prepared
cuts to a writer. Store/epoch/history/hash authentication, whole-batch budgets,
incoming lowering and common pending receipt persistence remain open. The
previous 13 pure `write_base` controls passed their separately admitted slot;
that result does not qualify this reader.

## Prepared batch bridge (source-stage)

`Store::read_prepared_slot_batch` connects immutable prepared cuts to the
indexed physical reader. Store identity, log epoch and captured head must match
both before and after the batch. ROOT uses its reserved IRI/sentinel; a missing
named graph term refuses. Subject and predicate dictionary lookups never intern
terms, and missing slot terms produce empty views without a graph scan.

The batch permits at most 256 explicitly prepared slots. Per-read limits remain
4096 physical claims / 4 MiB; totals across ALL slots and BOTH cuts are capped
at 65536 physical claims / 16 MiB. Actual scanned rows and encoded bytes are
charged before temporal filtering or visible-value deduplication. Each cut is
read separately, including equal legacy cuts. Budget exhaustion refuses the
whole batch; no partial views escape. A fully consumed budget can conservatively
refuse a subsequent empty read. Explicit empty scope performs no slot scans.

These checks do not acquire a writer reservation, authenticate the caller,
prove the declared retention watermark or verify the base slice hash. The
normal writer must still provide those guarantees. A matching before/after
head is not a claim that another writer was excluded. This source slice admits
canonical stable named subjects only; blank-node scope and generalized timestamp
precision remain open, together with incoming lowering and pending persistence.

Two source controls bind base/current views and their shared physical
budgets, including explicit empty scope, and refuse foreign store/graph or moved
head/epoch. Both passed in the scoped run at `6be9e447`, with a clean HOME guard.
The [retained transcript and limits](../demos/scoped-slot-reader/README.md)
describe that synthetic source target. This does not prove writer admission,
reservation exclusion, slice hash verification or installed behavior.

## Scoped common pending ledger (internal source slice)

An optional `scope` on the retained internal merge request now binds its schema,
ROOT graph, explicit canonical valid time and at most 256 named subject/predicate
slots (64 KiB of scope terms). The field is omitted for legacy requests, retaining
their existing identity encoding. Scoped requests are refused by public profiled
file ingress; this is not endpoint admission or a replacement for declared-base
slice authentication, retained-history proof or graph rollout policy.

The common pending executor and authenticated resume use bounded current-cut
snapshots for this scope. Base/current/incoming input graphs must fit inside it;
list-cell closures, other graphs, duplicate slots and out-of-scope units refuse.
Each snapshot uses the indexed physical reader with the existing aggregate
65536-claim/16-MiB budget across both cuts. No scoped branch calls the full ROOT
snapshot or its whole-subject guard. Multiple validation/persistence phases can
capture separate bounded snapshots; these limits are per snapshot, not a claim
about total admission latency or validator work.

Held-slot guards bind store/epoch, the exact scope and valid time, and a digest
of every bounded physical history row before visibility filtering or value
deduplication. An invisible received interval therefore invalidates resume even
when live values stay identical. Unrelated slots may change without invalidating
the retained remainder. Guard and snapshot data are evidence, not reservation,
authentication or retention capabilities. The existing atomic common ledger
continues to own received/decision/receipt ordering and rollback.

The real writer adapter, legacy policy/counters, declared-base hash/retention
admission, generalized time and blank-node support, full prospective validation
binding, and endpoint/performance acceptance remain separate open work. This
slice activates no server caller, graph policy, runtime or production database.

The existing planner provenance-start callback still reads current entity facts;
this slice does not claim all planner work is bounded. Only the common pending
snapshot/guard paths are replaced. Bounding that callback, authenticating declared
base slices and full writer integration must precede live admission.
