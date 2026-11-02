# Handoff startup boundary: enrollment is unavailable

This is the first Store increment of P1 in the
[handoff plan](ownership-handoff-plan.md). It reserves a fail-closed startup
boundary and preserves one commit-fence admission pin across callback replacement.
It does **not** complete P1 or implement source retirement, protected-journal
storage, generation admission, a witness, enrollment, or physical fencing.

## Contract check

The accepted [handoff design](ownership-handoff.md) requires agreement of the
protected journal, database marker, placement and expected authority tuple.
The journal must be outside the transferable/restored unit. A database copy or
sidecar cannot be that journal. Restoring the whole host also requires fresh,
nonce-bound authenticated external admission before an enrolled writer starts.
Model observation booleans and decoded candidate records supply neither control.

There is currently no authenticated startup adapter. Consequently **every
enrolled writable startup refuses**, including records claiming ACTIVE. This
increment supplies no authority-bearing allow flag, configuration switch,
public enrollment method, test signer or special transition token.

The reserved `main.ownership_handoff_enrollment` schema object is positive
handoff-enrollment evidence. Any object with that exact name, including an
empty table, view, malformed object or future schema, blocks ordinary writable
open before schema initialization and migrations. Deserialization/direct
initialization and explicit stage-A adoption use the same gate. The marker is
never created by a production constructor. Stage-A `ownership_enforcement` is
still a separate native-placement mechanism; it does not enroll a lineage.
Read-only inspection remains available without migration.

## Held handles and callback replacement

Native disk writers retain one `Arc<CommitFence>` containing a separate,
warmed read-only SQLite connection, a nonblocking check latch and an irreversible
per-handle legacy-admission pin. The ordinary installer and `set_commit_wake`
clone that same object. The wake installer cannot re-open a reader or replace
a revoked admission with a fresh one.

Each outer commit checks a fresh independent reader transaction. Ownership
inspection and write-entrypoint checks share the same pin: an early refusal
revokes it even when no commit hook runs. On observing enrollment, either path
permanently revokes that disk writer's legacy admission.
Removing/restoring the database marker afterward or reinstalling the callback
cannot revive it. Autocommit, explicit COMMIT and outermost RELEASE refuse;
the existing rollback-on-refused-release helper ends the doomed transaction.
A refused callback never invokes the wake hint. Check-latch contention,
unavailable readers and poisoned state refuse without waiting under SQLite's
writer lock.

This latch serializes admission **checks** only. SQLite's callback runs before
physical commit, so releasing a latch when the callback returns cannot prove
that it spans commit. It must not be reused as the future retirement latch.
No enrolled commit is admitted here. The future adapter must acquire a
per-lineage shared token before public write scopes and hold it through their
outer completion, use nonblocking direct-writer admission, and prove exclusive
retirement ordering and cross-resource recovery. The narrowly scoped retirement
transition token must not authorize unrelated facts.

## Qualification and remaining gates

Real private SQLite controls pair ordinary compatibility writes with enrolled
startup refusal before migration, deserialization refusal, held-handle
COMMIT/autocommit/RELEASE rollback, marker rollback after revocation, check-latch
contention and wake-hook replacement. These are library fixtures, not a native
cross-host handoff or an operational enrollment procedure.

A restored database predating the marker can still look unenrolled to a *new*
process. Closing that gap requires the external protected enrollment inventory
and startup adapter, not a filename inferred from the restored database. The
per-handle revocation here is not durable rollback protection. Cross-process
per-lineage latches, protected journal durability/recovery, generation pins,
real startup authentication, transitions, every writer/consumer and original
physical retained-writer acceptance remain open.

The protected journal's file adapter now exists
([`ownership_journal.rs`](../../src/store/ownership_journal.rs)): one model
`Journal` record with a sha256 trailer, an atomic fsynced replace, and a locked
advance that admits only Active to Retiring, Retiring to Retired and Pending to
Active. Anything unreadable is UNKNOWN.

A handle's commit fence can now carry a generation pin
([`ownership_fence.rs`](../../src/store/ownership_fence.rs)): the journal path
and the exact authority tuple the handle was admitted under. Every entrypoint
check and every outer commit re-reads the journal (a file read, never SQL on the
committing connection) and refuses unless it is still ACTIVE at that tuple.
RETIRING, RETIRED, PENDING, a different generation or epoch, and UNKNOWN all
refuse, and the first refusal revokes the handle for good, so restoring older
journal bytes cannot revive it. The handle keeps the role-specific reason, because
a refused COMMIT itself surfaces only as SQLite's generic constraint error.

No production path sets a pin: there is no enrollment API, no provisioned
journal location and no fresh external startup admission, so enrolled startup
still refuses before any journal is consulted. The pin is exercised by test
fixtures only. The per-lineage shared/exclusive latch spanning the outer COMMIT,
the narrow transition token and the startup adapter remain later P1 slices. Do
not enroll stores or claim at-most-one physical writer from this increment.
