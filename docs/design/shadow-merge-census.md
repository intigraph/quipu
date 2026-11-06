# Shadow merge census: retrospective source stage

`scripts/merge-shadow-census.py` is a bounded, read-only analyser of an explicitly
retained Quipu SQLite snapshot. It never installs a writer hook, creates a
snapshot, changes admission, or changes the source database. Do not point this
source-stage tool at the live writer. An owner-bound retained snapshot and a
read-cost scope are required before collecting the production corpus.

```sh
just shadow-census inspect --snapshot /absolute/retained.sqlite \
  --after-tx 100 --through-tx 200 --max-pairs 5000 --gap-seconds 300
just shadow-census test
```

The analyser joins an old physical assertion's actual `retracted_tx` to a new,
different assertion of the same graph/entity/predicate in that transaction.
Both intervals must be nonempty when parsed as instants. Different declared
actors and a bounded transaction time gap select **candidates**, not proven
lost updates. Nearby writes without that closure link do not count. Legacy
closures without a recorded retracting transaction are unobservable, not zero.
The schema's retained-retraction and transaction indexes are required; the
analyser refuses an older schema rather than falling back to a full scan.

The explicit transaction window is `(after_tx, through_tx]` on replacement
transactions. Earlier assertions can contribute when their closure is in that
window. One SQLite read transaction binds store identity, log epoch, snapshot
head and rows. The default limit is 5,000 physical pairs, hard-capped at 50,000;
an extra row refuses the entire result. A five-second query/analysis deadline
(at most ten seconds through the library interface) also refuses. No partial
sample is advertised as a complete bounded result. CLI failures emit a static
`unknown` response without private paths or database diagnostics.

Output groups numeric graph and predicate IDs and counts candidate slot
transactions. It contains no entity IDs, fact values, actor strings or query
text. Physical source claims may duplicate one logical replacement; physical
pair counts explicitly do not claim logical-slot deduplication. Graph IDs do
not establish ontology membership. Multiple ontologies may share one graph.

When the retained snapshot contains the current local `transaction_auth`
table, each candidate pair separately reports `same_recorded_principal`,
`different_recorded_principals` or `unknown`. The declared-actor candidate
filter and its counts stay separate. Different actor strings can share one
credential principal; neither equality nor difference proves which individual
agent wrote. Missing legacy authentication tables and missing/blank evidence
remain explicitly unknown. A present table without the expected columns and
unique transaction primary key refuses the result rather than multiplying
pairs. Joins use that primary key and remain inside the same bounded read
transaction. Raw principal and authentication-class strings never leave the
analyser. This describes recorded local provenance, not cryptographic signature
re-verification, current credential validity or a grant of writer authority.
Same-declared-actor pairs remain outside this retrospective candidate cohort,
even if their recorded principals differ; this is not a complete identity census.

Sizing output includes fixed time-gap buckets and a hot-slot histogram of
distinct replacement transactions per physical graph/entity/predicate slot.
Multiple old/new claims in one replacement can produce a pair cross-product;
that inflates physical pair counts but does not inflate this histogram. Neither
entity IDs nor timestamps are exported. Per-group distinct slot/transaction
counts reconcile to the overall count. These are retained-window physical
sizing observations, not an entity lookup, ontology assignment or proof that a
slot should become a fluent.

The retrospective stage is **not** a second implementation of `merge3` and does not provide its
three-way classification. Read bases, shape/cardinality snapshots, class and
ontology mapping, individual agent attribution and LWW-loss evidence remain unknown.
The live shadow stage must call the existing merge core against a declared
base, preserve normal write behavior, and bind its classification to the actual
shape/source version. It still needs reviewed source, normal qualification,
bounded owner-admitted collection, per-ontology reconciliation and coverage
evidence. Neither synthetic tests nor this source stage complete the census.

## Native exact-base source adapter (not installed)

`merge_core::census::observe_batch` calls the existing `plan_slot` with its
default decision. It contains no alternate merge rule and executes none of the
returned fact operations. Native classifications are `unchanged`, `ours`,
`theirs`, `convergent`, `divergent`, `maxcount-exceeded` and `delete-replace`;
the two one-sided classes remain distinguishable. A per-slot hold is not a
claim about what an atomic batch would commit.

The source producer supplies an explicit base-evidence kind and whether the
shape bound is known. A missing base or unknown bound emits an unclassified
row, with neither a clean classification nor a false hold result. Known
unbounded cardinality is represented separately from unknown cardinality.
Evidence labels describe the producer's input; they do not authenticate it or
prove that a supplied bound matches the actual shape revision.

Before planning, the adapter refuses an entire batch above 256 slots, 4,096
value occurrences or 4 MiB of term/time input bytes. Output contains only
graph/predicate IDs, evidence knownness, native classification and the default
per-slot hold. It excludes entity IDs, values and timestamps. There is no
writer hook, persistent event sink, environment activation or live read in this
adapter. Its Rust tests are source-only until a Cargo START qualifies them.

Live integration must derive base and shape inputs inside the existing writer
reservation, after authority checks, without changing success/refusal behavior.
`store/conditional.rs::check_expected` already reserves the writer before
reading explicit single-slot expectations; an expectation supplies values but
does not itself prove an actual `sh:maxCount` bound or a read transaction.
Ordinary writes without a base remain unknown. Ontology/class assignment must
be resolved from a content-bound profile, outside this value-free adapter;
graph membership and caller-declared actors cannot substitute for it. A bounded
nonblocking sink with explicit dropped/unknown counts, producer identity and
shape-version binding still needs implementation and normal deployment.

## Bounded aggregation source follow-up (not installed)

`merge_core::census::aggregate::AggregateSink` is an in-memory source-stage
consumer, without a writer hook, schedule, persistence or export endpoint.
`try_record` never waits for its mutex. Busy or poisoned state, malformed reports,
oversized input and exhausted bucket capacity drop the whole batch with an
explicit health count. Capacity is 256 distinct buckets and at most 256 reports
per call. A capacity refusal cannot increment existing buckets from that batch;
existing buckets can still receive later observations when capacity is full.

Buckets separate producer source revision and optional profile revision, graph,
predicate, base evidence, known/unknown cardinality, native classification and
per-slot hold. Revisions are caller-supplied evidence, not authentication or proof
that the writer used that profile. No entity IDs, terms, authored timestamps or
free-text unknown reasons enter the sink. Counts are slot observations, not
distinct entities or a durable delivery ledger. Missing profile evidence remains
explicit; it is never replaced by a guessed revision.

Snapshot reads also use `try_lock`: busy and poisoned reads are explicit, not
empty successful snapshots. Accepted state is sampled under the same lock;
drop health counters are independently sampled and labelled as such. All counts
saturate rather than wrapping, and saturation is reported. This is no claim of
complete capture, atomic health reconciliation, process-restart retention or
writer behavior. The producer must still bind real authority/base/profile inputs,
account for native planning refusal, and connect the source to a separately
admitted durable consumer. New aggregation tests await a Cargo START.


## Default-off observation bridge (source only, unqualified)

`AggregateSink::try_observe` connects the existing bounded native observer to
the existing aggregate sink without calling a writer. `ShadowMode` defaults to
`Disabled`; that path skips planning and increments a disabled-batch health
counter. Explicit `Observe` counts whole-batch planning refusal separately from
sink input, capacity, contention and poison drops. Planning errors are discarded
after counting; private term bytes and diagnostics are never placed in health
output. All counters use the same saturating path and independent-sampling caveat.
Native unknown inputs remain unknown observations, not successful classifications.

The bridge returns observation outcomes rather than a writer `Result`. A future
caller must preserve the original writer success/refusal and must not propagate
observer outcomes into admission. No configuration, writer invocation, sink
export or live-store read is installed by this change. Bounded input planning
and a nonwaiting mutex do not prove a wall-clock or writer-latency budget;
producer capture cost and worst-case qualification remain open. The two new
bridge tests are unrun pending a separate Cargo START.


## Writer capture and commit evidence (design, not wired)

The source adapter observes hypothetical merge decisions; its accepted-batch
counter is not a committed-write counter. Live integration must retain that
meaning. The current server source gives three concrete boundaries:

- `StoreHandle::lock` settles signed attestation under the existing writer
  lock; `metadata_lock` explicitly does not. Do not acquire the latter to avoid
  nonce settlement, or settle a second time for census observation. Rejecting
  authority must preserve existing write behavior and cannot create a census
  classification that implies authorized application.
- `StoreHandle::commit_wake_for` installs a `watch` notification carrying `()`.
  It is a coalesced commit hint, not a per-write receipt, transaction identifier
  or exactly-once census stream. Polling current state after that hint cannot
  reconstruct the request's original base/profile inputs.
- `AppError::message` documents failures after earlier committed steps. HTTP
  status, successful planning and client delivery therefore cannot establish
  commit or rollback. A disconnected client is not proof a write was unrun.

The future producer captures bounded touched-slot inputs and immutable profile
identity under the normal authenticated reservation, before mutation, and tags
that work as a classification observation only. No new SQLite read or planner
work may be smuggled into a commit hook. The producer must bind an eventual
committed outcome to actual store transaction evidence, or report outcome
unknown. Only that separate evidence permits claims about received or applied
writes. Explicit rollback/refusal, no-op and unknown outcome must stay separate;
do not count all observer `Accepted` results as overwrites. Numeric transaction
identifiers remain private join evidence, outside aggregate labels.

A bounded staged observation must be discarded or accounted for on rollback,
with no partial publication advertised as complete. If classification occurs
only after commit, the producer must have captured the immutable pre-write
inputs first; re-reading current slots afterward is not an equivalent base.
Overflow, busy sink, classification refusal and commit-evidence loss remain
visible coverage gaps. The current in-memory bridge does not supply an atomic
commit/outbox join, replay identity or process-restart retention. Do not add an
unbounded queue or block the writer to hide those gaps.

Future integration controls must include successful commit with lost client
response, a refusal before a data transaction, rollback after classification,
no-op with attestation settlement, two commits coalescing into one wake, and
process loss between commit and observation delivery. Each must preserve the
writer's original return/authority behavior and show explicit census coverage
or unknownness. This design adds no hook, transaction, endpoint or activation.

## Consuming staged capture (source only, unqualified)

`AggregateSink::try_stage` now owns the bounded value-free reports and their
producer binding before mutation. Request data may then be dropped or changed;
completion never re-reads it. Disabled mode still skips all planning. The
staged object is not clonable and completion consumes it; there is no queue,
commit hook or new SQL work. This module remains under the existing test-only
census gate, with no production writer caller.

The future adapter supplies a separate write outcome. A declared committed
transaction must be positive, but is caller evidence rather than authenticated
proof. No-data-change remains distinct. Rollback and unknown outcome discard
the classifications and increment separate health counters. Dropping an
unfinished stage accounts for in-process abandonment. A declared commit
still reports sink contention, poison or capacity loss independently: its
writer outcome cannot be changed by observation failure. Transaction IDs
remain private join evidence outside aggregate keys and snapshots.

These counters remain independently sampled classification/coverage evidence,
not committed-write counts or a durable outcome ledger. In-process ownership
does not give restart retention or exactly-once publication; process death
before `Drop` remains unobserved. Real reservation/authentication, profile
validation, store transaction evidence, durable delivery and crash recovery
are still required before live integration. Four new controls for owned
capture, rollback/unknown/abandonment, no-op versus commit/contention, and
disabled/refused planning are source-only and unrun. The previous eight
aggregate controls passed the earlier admitted combined slot; that result
does not qualify this extension.
