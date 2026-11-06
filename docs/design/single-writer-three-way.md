# Single-writer three-way admission

Status: source preparation. No server write path uses this proposal. The
`write_base` module only binds coordinates; it neither validates a slice digest
nor acquires a writer reservation. Its tests are pending qualification.

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


## Prepared read cuts (source checkpoint, unqualified)

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
