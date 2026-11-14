# Private P1 commit scopes

This source slice extends the [handoff plan](ownership-handoff-plan.md)'s P1
boundary. It supplies a private commit seam and private native fixtures. It
does not enroll a lineage, admit startup, select a witness or journal location,
activate enforcement, or establish physical two-host fencing.

## Lifetime, rather than a callback check

The existing `CommitFence` re-reads the pinned journal at the SQLite pre-commit
callback. A check that releases its lock when that callback returns cannot
order retirement against the physical commit that follows.

`ownership_latch` instead supplies a shared permit for a commit scope and an
exclusive, path-bound journal transition token. Acquisition is nonblocking.
Both use the same sibling lock file. The shared permit is acquired **before**
the journal is checked and before entering SQLite; it survives the actual SQL
call, including COMMIT or outer RELEASE, and cleanup. Journal transitions hold
the exclusive token through expected-role comparison, legal-transition checks,
atomic rename and directory fsync. A callback never waits for retirement while
holding SQLite's writer lock.

On Unix, journal, lock and staged files must be owner-only regular files, with
no symlink traversal or additional hard links. The parent directory is still
an assumed protected namespace. This is not protection against a privileged
owner replacing the entire directory, lock or host state. Whole-host rollback
still needs fresh external startup admission.

## Private Store seams

`ownership_operation` frames **one existing atomic Store operation**, including
registry/share writers that require their own independent transaction. It
holds the permit until that operation returns from its real SQL completion.
It does not turn a sequence of separately committing APIs into one atomic
transaction, or roll back a transaction that has already durably committed.
The existing API's post-commit error and lost-response semantics remain.

`ownership_transaction` owns an outer savepoint for savepoint-compatible work.
Its body may not commit that outer scope early: the same SQLite commit fence
requires the permit's completion phase. Only the final RELEASE may commit.
An error or unwind rolls back its unfinished transaction, discards provisional
term/read-model caches and pending evidence, and returns no success receipt.
It refuses to take over a caller's pre-existing outer transaction. An operation
that leaves its own transaction unfinished also refuses and cleans it up.

Pinned SQL outside a permit refuses even while its journal says ACTIVE.
RETIRING, RETIRED, PENDING, UNKNOWN and tuple mismatches still revoke the
generation pin permanently. A temporary exclusive-latch conflict refuses the
current operation without pretending to establish authority. The ordinary
installer and wake-hook replacement share the same fence.

There is no unsafe SQLite trace/profile hook or replacement connection API.
The seams are private and have no production callers; only the existing test
fixture can set a generation pin. Unpinned INACTIVE compatibility is unchanged,
and enrolled startup still refuses. A future admitted startup/transition slice
must supply real authenticated admission and route each writer through its
appropriate seam. This source does not silently enable that route.

## Draft proof controls and limits

The draft uses real SQLite autocommit, explicit COMMIT and outer RELEASE, plus
the actual Store transact, batch, registry, cursor, subscription, RDF import,
share import and fork-promotion APIs. Refusals have ordinary positive controls.
Offline import opens its own raw connection: its existing cold enrolled-startup
gate is tested separately, rather than being authorized by another connection's
permit. A complete table-value snapshot checks rollback residue, including
facts, cursors, audit and receipt rows; provisional term IDs are checked too.

Deterministic bounded pauses inside the real commit callback and after the
physical SQL call must keep exclusive retirement out until the shared permit
is released. The latter pause observes the committed row from an independent
reader. A Unix subprocess checks an independent process's exclusive latch
against the actual Store seam. These remain private, local fixtures, not P5.

Required qualification counterfactuals remove the actual shared latch hold and
the actual scope backstop independently; each must fail its discriminating
control before exact source restoration returns green. Draft controls are not
a passing qualification receipt. Fresh scoped Cargo qualification, all normal
quality/build surfaces, complete CI and one non-author source review remain
required. P2/P3 startup/retirement recovery and physical P5/P6 stay separate.
