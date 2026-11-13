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
fixtures only. The source-only [private P1 commit scope](ownership-commit-scope.md)
adds the shared/exclusive lineage latch, path-bound transition token and
explicit SQL completion seam; a pinned commit outside that seam refuses.
Its private fixtures do not activate public writers or supply the startup
adapter. Do not enroll stores or claim at-most-one physical writer from it.

## S1: authenticated enrolled startup (contract for the next increment)

Status: source contract for slice S1 of the
[witness and enrollment design](ownership-witness-enrollment.md) (D3). It
replaces "every enrolled writable startup refuses" with "an enrolled writable
handle exists only through this route". Nothing here enrolls a store, selects
a witness bucket or principal, or wires a production caller.

**Marker storage.** E1 defined the marker body (`Binding`), not its storage.
An admissible marker is exactly
`CREATE TABLE ownership_handoff_enrollment(id INTEGER PRIMARY KEY CHECK (id = 1), binding TEXT NOT NULL)`
holding one row whose `binding` is the canonical E1 JSON. Any other object of
that name, schema, row count or body is `Marker::Unknown` and refuses. The
ordinary route keeps refusing every object of that name, exactly as today.

**Route.** A separate native-only `Store::open_enrolled(path, &EnrolledStartup)`
takes an explicit inventory path and a witness source that resolves the
inventory's profile name to an `AuthorityWitness` backend. Ordinary `open`,
deserialization, `open_owned` and stage-A `adopt` are unchanged and keep
refusing enrolled databases. Browser/wasm builds refuse the route.

**Admission sequence**, every step `Unknown` on doubt, and any `Unknown` means
no handle:

1. Canonicalize the path, open it without following links, require a regular
   file with one link, and take device/inode from that open file.
2. Read the marker without migration; decode it strictly.
3. Read the configured inventory: absolute, regular, not a link, owned by the
   current user, not group- or world-writable. Decide with E1; only
   `NeedsJournalAndWitness` with the matching record continues.
4. Require the record's journal to be a protected file of the same kind, not
   the database or one of its sidecars.
5. Acquire the SAME shared lineage latch the commit seam uses, and hold it
   until the pin is installed.
6. Read the journal. Only `ACTIVE(g)` for the record's store and unit, with
   `g >= enrolled_at_generation`, continues. RETIRING/RETIRED/PENDING refuse
   ordinary startup; the transition path owns them.
7. Draw a fresh 32-byte nonce, then read the witness head anchored at `g`
   through the resolved backend, passing that nonce to `WitnessSource` and
   keeping every object version read. Record the same nonce in the receipt.
8. Apply the D3 table. Only `Active(g)` with the identical authority tuple
   admits. `Reserved(g)` refuses ordinary startup. `Retired(g)`, `Active(g+1)`
   or later refuse as a rolled-back or superseded journal.
9. Re-read the journal under the same latch; it must be byte-identical.
10. Write the startup receipt (below). A write, fsync, rename, directory
    fsync or read-back failure means no handle.
11. Construct the store through a private initializer that accepts only an
    `Admission`, then install the generation pin and the admitted binding.

**Admission value.** `Admission` has private fields, no public constructor and
no `Clone`, `Default`, `Deserialize` or test setter in production builds. It
is created only at step 11 of `admit` and consumed by `CommitFence::pin_admitted`,
which accepts the opaque proof rather than raw pin fields. Its startup latch
is retained until installation finishes. Commit-boundary test fixtures enroll
disposable files and use this same admission route; there is no test pin setter.
No boolean, decoded record, receipt or caller value can stand in for it.

**Commit-time check.** `state_on` today revokes and refuses whenever the
marker exists. With S1 it admits an enrolled database only when the handle's
fence carries an admitted binding and the marker read on that connection
decodes to the same `store_id` and inventory digest. Any absent, changed or
unreadable marker revokes the pin for good, as today. The generation pin's
journal re-read runs first on every check, unchanged. Stage-A enforcement
checks after that are unchanged. The witness is not called per commit: a
running admitted owner continues through a witness outage, and only startup
and handoff need the witness.

**Receipt.** `<journal>.startup-receipt`, a sibling in the journal's protected
directory, holding canonical JSON: version, database path/device/inode,
inventory record digest, journal bytes digest, authority tuple, witness
profile name, the witness object versions read, nonce, and admission time.
It holds no credential. It is audit evidence, never authority: no startup
reads it as admission, and a replayed receipt cannot replace the live
witness read.

**Still refusing after S1.** The WAL checkpointer admits an enrolled database
only through the same admitted fence; otherwise it refuses as today.
`open_owned`, `adopt`, deserialization and file-replacing operations (restore,
compaction, migration copy) keep refusing enrolled databases until each is
inventoried as a transition (D1). Enrollment (T1), transitions (D4), a
production witness-profile resolver, and the deployment decisions (witness
principal and storage, inventory path, break-glass) are outside S1.

**Qualification.** Every D3 row; a restored old journal with the witness
ahead; a witness outage, slow witness and malformed or versionless witness at
startup; a running owner during a witness outage; nonce and versions recorded;
no pin when any step is `Unknown`; path, device, inode, copy and hard-link
mismatches; link traversal and permissive modes; receipt durability failure;
a marker changed after admission; and a commit outside the seam. One positive
fixture traverses the same non-test route over the W2 backend and emulator.
Mutants that expose a test pin, skip the witness or tuple comparison, skip the
receipt fsync, or drop the latch before installation must each fail a named
control.
