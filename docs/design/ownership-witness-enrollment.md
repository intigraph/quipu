# Ownership handoff: enrollment inventory, chain-head witness and startup admission

Status: proposed design. It fills the control-plane gaps that
[explicit source retirement](ownership-handoff.md) leaves as prerequisites:
where an enrolled store's protected journal lives, what the external chain-head
witness is, how an enrolled writer is admitted at startup, and who may drive a
transition. It selects no principal, provisions no witness, and enrolls no
lineage. Each section ends with the source slice that would implement it.

Option A from the handoff design is unchanged: the source retires at its own
commit boundary before any receipt exists, and there is no lease, timeout or
clock-based takeover. This document adds no new authority path. It only makes
the existing prerequisites concrete enough to build and test.

## Problem being closed

The retained two-host fixture (Host A writer open, store adopted on Host B)
showed that B's fresh epoch and host/path binding do not stop A: an
already-open A handle committed after B's adoption. The landed slices give A's
handles a generation pin, a per-lineage latch and a protected journal, so a
retirement that A performs is enforced on every A handle. Three things are
still missing before any enrolled writer can run:

1. **Location.** Startup cannot find a store's journal. Deriving it from the
   database path is forbidden ([startup boundary](ownership-startup-boundary.md)),
   because a restored database must not choose its own authority file.
2. **Rollback detection.** A whole-host restore brings back an old journal with
   valid bytes. Only state outside the restored unit can detect that.
3. **Transition authority.** Retirement, destination admission and enrollment
   need a caller that ordinary write paths cannot obtain.

## D1. Enrollment inventory

### Contents

Each host that may run an enrolled writer has one **enrollment inventory**: a
small versioned file, one record per enrolled lineage. Encoding follows the
journal adapter: strict canonical body, sha256 trailer, atomic replace with
directory fsync, and an exclusive sibling lock for changes.

| Field | Meaning |
| --- | --- |
| `store_id`, `unit` | The lineage and authority unit, as in `Authority` |
| `database` | Canonical absolute path **and** the device/inode observed at enrollment |
| `journal` | Absolute path of this lineage's protected journal |
| `witness` | Name of an admitted witness profile (D2), never a URL or credential |
| `enrolled_at_generation` | Generation recorded when the lineage was enrolled here |

The inventory location comes only from explicit server configuration
(`[quipu.ownership] inventory = "<absolute path>"`). There is no default and no
search path. The inventory, every journal and their directories are owned by
the service user with modes 0600/0700. They live outside the database
directory and outside every backup or transfer set. Deployment must exclude
them from backups explicitly. Restoring them together with the database is the
whole-host rollback that D2 exists to detect.

### Two-sided binding

The database keeps the reserved `main.ownership_handoff_enrollment` object.
In this design it carries the `store_id` and the digest of the inventory
record that enrolled it, and no path. Startup requires both sides to agree:

| Database marker | Inventory record for this file | Result |
| --- | --- | --- |
| absent | none | Legacy, unenrolled, unchanged behavior |
| present | present, `store_id`, digest and device/inode match | Continue to the journal and witness (D3) |
| present | missing, or the inventory is not configured | `UNKNOWN`: refuse writable open |
| present | present but a different digest, `store_id` or device/inode | `UNKNOWN`: refuse (copied or replaced file) |
| absent | a record names this path or device/inode | Refuse: an enrolled store was replaced by an unenrolled copy |

The last row stops the downgrade in which someone deletes the marker or swaps
in an old unenrolled backup to get legacy behavior back. A copied file
carries the marker but not the inode, so a copy never inherits the original's
enrollment.

Binding to the device/inode has a cost: any operation that replaces the
database file (a restore, a compaction that writes a new file and renames it,
a migration copy) changes the inode. For an enrolled store, each such
operation must run as a transition (D4) that rewrites the inventory record
under the token, or it must refuse. The store maintainer must inventory every
file-replacing code path before E1 is wired into production constructors.
An unlisted path fails closed: the next startup refuses.

Slice **E1** (store maintainer): inventory codec and reader in the style of
`ownership_journal`, plus the marker body and the decision table above. It is
not yet called by production constructors. Tests cover every row, a renamed
file, a hard-linked copy, a copied file, a corrupt trailer, an unconfigured
inventory and an oversized record.

## D2. Chain-head witness

### What the witness must provide

From the handoff design and the executable model:

- **Durability outside the restored unit.** Restoring a host must not restore
  the witness.
- **Serialization.** At most one successor reservation per predecessor, so
  concurrent B/C requests produce exactly one winner.
- **Phases.** `ACTIVE`, `RESERVED` and `RETIRED`, as in the model's
  `WitnessHead`, not just an owner tuple.
- **Freshness.** A startup check must observe the witness's current state, not
  a replayed answer.
- **No new trust root.** The witness uses an already admitted identity.

It must **not** be able to supersede a running owner. It records what the
owner did; it grants nothing by itself.

### Options

| Option | Assessment |
| --- | --- |
| Another Quipu store as witness | Circular: the fenced system would vouch for itself, and it needs its own rollback story. Not selected |
| Signed append-only git history | Gives attribution, but has no native compare-and-swap, and force-push or administrative rewrite falls inside the fault window. Not selected |
| Lease or consensus service | New infrastructure, and lease semantics that Option A rejected. Not selected |
| Object storage with conditional create and strong read-after-write consistency | Create-only keys give one winner per key; durability and freshness come from a store outside the host; it reuses an existing service principal. **Selected as the first adapter** |

### Record layout

The witness is an append-only set of immutable records. A generation has one
`ACTIVE` record and a sequence of handoff **attempts** `k = 0, 1, ...`, each
with a reservation and at most one outcome:

```text
<prefix>/<store_id>/g<g>/ACTIVE             body: Authority (genesis or successor)
<prefix>/<store_id>/g<g>/r<k>/RESERVED      body: Request (predecessor g, exact destination, operation ID)
<prefix>/<store_id>/g<g>/r<k>/OUTCOME       body: RETIRED { Certificate } | ABANDONED { request digest, operator, reason }
<prefix>/<store_id>/g<g+1>/ACTIVE           body: Authority (successor named by the RETIRED certificate)
```

A single `OUTCOME` key per attempt makes retirement and abandonment mutually
exclusive at the witness: whichever create wins is the attempt's only
outcome. A reservation is never rewritten or re-targeted. A different
destination, or a retry after abandonment, is a new attempt `k+1` with its own
operation ID. Attempt `k+1` may exist only after attempt `k` has an
`ABANDONED` outcome.

Every write is **create-only** (conditional on the key not existing). Nothing
is ever overwritten or deleted by Quipu. The storage location must also
retain records: no lifecycle expiry, and versioning or object lock where
available. A deleted record makes the head walk stop early. The result is
"head behind the journal", which refuses (fail closed) but blocks that
lineage's startup until reconciled. A conditional create that loses returns a
conflict; it never silently overwrites.

### Head walk and its anchor

The walk starts at an **anchor generation chosen by the reader's journal
role**, never blindly at the journal's own generation:

| Journal role | Anchor | Why |
| --- | --- | --- |
| `ACTIVE(g)`, `RETIRING` or `RETIRED` at `g` | `g` | The journal names the generation it holds or is leaving |
| `PENDING(g+1)` | `g`, the predecessor named in the journal's retained certificate | A pending destination has no `ACTIVE(g+1)` yet; its authority comes from `g`'s outcome |
| none (enrollment) | `0` | Genesis |

From the anchor `a`, the walk reads `g<a>/ACTIVE`, then attempts `k = 0, 1, ...`.
It continues past an `ABANDONED` outcome, and stops at the first attempt
whose `RESERVED` is absent, whose `OUTCOME` is absent, or whose `OUTCOME` is
`RETIRED`. After a `RETIRED` outcome it reads `g<a+1>/ACTIVE`, and if that
exists it continues the same walk at `a+1`. The walk also reads one key past
its stopping point. A `RESERVED` at attempt `k+1` without an `ABANDONED`
outcome at `k`, any attempt after a `RETIRED` outcome, or an `ACTIVE(a+1)`
without a `RETIRED` outcome naming it, is an illegal sequence. Object storage
cannot refuse those writes, so the walker reports them as `UNKNOWN`
(tampering or a defective writer), never as a head.

With strong read-after-write consistency the walk is exact. Its result maps
onto `WitnessHead`:
- `ACTIVE(g)` with no unresolved attempt is `Active`.
- A `RESERVED` attempt without an `OUTCOME` is `Reserved`.
- A `RETIRED` outcome without `ACTIVE(g+1)` is `Retired`.
- `ACTIVE(g+1)` is the successor's `Active`.
- An `ABANDONED` outcome returns the head to `Active(g)` for the same tuple.

**Model change required before W1:** the accepted model (#939) has no abandon
transition. P0 must add an `Abandon` action, `Reserved -> Active` at the same
tuple, guarded by the source journal still being `ACTIVE(g)`, and re-run the
state enumeration. Two active owners and a stale-B activation must remain
unreachable. The model's rule that a request's destination is never its
source stays unchanged: abandonment is not a self-handoff.

### Freshness and the startup nonce

Startup generates a fresh nonce and performs authenticated reads of the head
keys over an authenticated transport, recording each object's version
identifier. The nonce, the observed versions and the head are written to the
startup audit record (the model's `Startup` payload). The response cannot come
from a restored host because it is read live from outside it.

**Stated limitation:** the first adapter does not sign witness responses. A
privileged operator holding the witness credential could forge witness
records. That is the same privileged-operator exclusion the handoff design
already states. A signing witness can replace the adapter later without
changing the protocol.

### Witness interface

`AuthorityWitness` is a trait with four operations, each returning
`Ok`, `Conflict` or `Unknown`, never a guess:

- `head(store_id, anchor)`: the anchored head walk.
- `reserve(request)`: create `g/r<k>/RESERVED` for the next attempt `k`. A
  concurrent reserver for the same `k` gets `Conflict`.
- `retire(certificate)`: create `g/r<k>/OUTCOME = RETIRED`; the certificate
  must bind attempt `k`'s exact request.
- `abandon(attempt, reason)`: create `g/r<k>/OUTCOME = ABANDONED`. This is a
  source-side transition only (D4): it is admitted only while the source
  journal is still `ACTIVE(g)`, that is, before step 2.
- `activate(authority)`: create `g<g+1>/ACTIVE`; requires a `RETIRED` outcome
  whose certificate names this destination and this successor tuple.

An `Unknown` result (timeout, transport error, unexpected status, unreadable
body) never advances a local state. Lost replies are resolved by reading the
exact key back: the same body means the operation happened, a different body
means a conflict.

Slice **W1** (signing/authority maintainer with the store maintainer):
the trait, an in-memory adapter with deterministic fault injection (lost
reply, conflict, unknown, slow), and the head-walk mapping to `WitnessHead`.
No network adapter yet. Slice **W2**: the object-storage adapter behind a
feature flag, with conformance tests that run the same suite as the in-memory
adapter against a local conditional-create emulator. Selecting a real bucket
and principal is a deployment decision, not part of either slice.

## Ordering of journal and witness writes

Each transition writes the witness and the protected journal in a fixed order.
A crash between any two steps leaves no owner, or the same owner, never two.

| Step | Source A | Witness | Destination B |
| --- | --- | --- | --- |
| 1 | | `reserve`: create `g/r<k>/RESERVED` | |
| 2 | journal `ACTIVE(g)` -> `RETIRING`; held handles refuse from here | | |
| 3 | seal cut, commit retirement record and outbox; journal -> `RETIRED` | | |
| 4 | | `retire`: create `g/r<k>/OUTCOME = RETIRED` | |
| 5 | release the receipt from the outbox | | |
| 6 | | | journal `PENDING`; install and verify the artifact |
| 7 | | `activate`: create `ACTIVE(g+1)` | |
| 8 | | | journal -> `ACTIVE(g+1)` |

Crash analysis:

- **After 1, before 2.** The witness says `Reserved`. A running A keeps
  writing (the design allows an admitted A to continue until its barrier). A
  *restarted* A refuses, because startup sees `Reserved`. Recovery has exactly
  two exits, both through the transition path on A:
  - **resume** the identical operation to the same destination;
  - **abandon** the attempt with `OUTCOME = ABANDONED`. This is admitted only
    while A's journal is still `ACTIVE(g)`. Afterwards the head is `Active(g)`
    and A may restart.

  After abandonment, B's attempt-`k` request can never activate, because
  activation requires a `RETIRED` outcome. A later handoff is attempt `k+1`.
  No receipt exists before step 4, so B cannot have reached `PENDING`.
- **A restored from backup.** Suppose a whole-host restore brings A's journal
  back to `ACTIVE(g)` after A had crossed step 2 or 3 but before step 4. An
  abandon then loses A's commits after the restore point, which is ordinary
  rollback loss. It cannot create a second owner: no `RETIRED` outcome
  exists, so no receipt was ever released.
- **After 2 or 3, before 4.** A is fenced locally. No receipt exists, so B
  cannot activate. Recovery retries step 4 with the identical certificate.
- **After 4, before 5 or 6.** No owner is active. B can fetch the same receipt.
- **After 6, before 7.** B is `PENDING(g+1)` and refuses writes. Its walk is
  anchored at `g` (its certificate's predecessor), finds the `RETIRED` outcome
  equal to its retained certificate and naming B, and finds no `ACTIVE(g+1)`.
  That is the legitimate pending state, so the transition path retries
  `activate`, which is idempotent by key.
- **After 7, before 8.** The witness names B, but B's journal is still
  `PENDING`, so B refuses writes. B's startup sees `ACTIVE(g+1)` naming its own
  authority and completes step 8. Any other host claiming `g+1` sees a
  conflict.

Concurrent B and C: only one `g/r<k>/RESERVED` can be created. The loser gets
`Conflict` at step 1 and stays a non-owner.

Retirement against abandonment: both are source-side and run under A's
exclusive latch, and abandon is refused once A's journal leaves `ACTIVE(g)`.
If both are attempted anyway, the single create-only `OUTCOME` key admits
exactly one.

## D3. Startup adapter

An enrolled writable open runs these checks before admitting any write,
after the D1 binding succeeds:

1. Read the protected journal. `Unknown` refuses.
2. Head-walk the witness from the anchor its journal role selects (see "Head
   walk and its anchor"), with a fresh nonce. `Unknown`, including an outage
   or an illegal sequence, refuses writable startup.
3. Compare:

| Journal | Witness head | Result |
| --- | --- | --- |
| `ACTIVE(g)` | `Active(g)`, same tuple | Admit: install the generation pin and latch, record the startup audit |
| `ACTIVE(g)` | `Reserved(g)` | Refuse ordinary startup; only the transition path may resume the reserved operation |
| `ACTIVE(g)` | `Retired(g)` or `Active(g+1)` or later | Refuse: this journal was rolled back or superseded; the store is a non-owner |
| `RETIRING` or `RETIRED` | any | Refuse writes; the transition path may finish the identical retirement |
| `PENDING(g+1)` | anchored at `g`: `Retired(g)` whose certificate equals the journal's and names this destination | Refuse writes; the transition path may run `activate` (legitimate crash after step 6) |
| `PENDING(g+1)` | `Active(g+1)`, same successor | Transition path completes journal `ACTIVE`; then admit (crash after step 7) |
| `PENDING(g+1)` | predecessor outcome absent, `ABANDONED`, or a certificate that differs or names another destination | `UNKNOWN`: refuse |
| any | the anchor's own records missing (`ACTIVE(g)` for an `ACTIVE`/`RETIRING`/`RETIRED` journal; the predecessor `RETIRED` outcome for `PENDING`) | `UNKNOWN`: refuse. The journal never gets ahead of its anchor under this protocol, so this means witness loss or tampering |

A running, already admitted owner does **not** consult the witness per commit.
A witness outage blocks startup and handoff, not running commits. This is the
availability tradeoff the handoff design chose.

Slice **S1** (store maintainer): the adapter, wired into the existing
enrolled-startup refusal so that a pinned handle can be produced only through
this path. Tests: every row; a restored old journal with the witness ahead; an
outage at startup; a running owner during an outage; nonce and version
recorded; no pin when any check is `UNKNOWN`.

## D4. Transition authority

The transition token stays a crate-private type. It is constructed only inside
four operations: `enroll`, `retire`, `abandon` and `admit`. There is no HTTP route for
any of them in this design. They are exposed only as an operator CLI that:

- requires the exclusive per-lineage latch, which also proves no server process
  holds the store writable;
- requires every witness operation it depends on to return `Ok`, not `Unknown`;
- writes an audit record naming the operator principal, operation ID and
  witness versions.

**Enroll** creates the witness genesis `ACTIVE(0)` (create-only), then the
journal, then the inventory record, then the database marker, in that order. Because genesis is create-only, a lineage cannot be enrolled twice, on the
same host or another one: the second attempt gets `Conflict`. A
crash before the marker leaves an unenrolled database plus an orphan record.
Startup refuses (last row of D1) until the operator finishes or removes the
record.

Slice **T1**: the CLI and token plumbing over W1's in-memory adapter.
No production principal is involved.

## The case this does not cover: Host A is permanently lost

Option A gives no takeover while A might still be running. If A's host is
destroyed, its lineage cannot be handed off by protocol. A break-glass
retirement would have to create a reservation and a `RETIRED` outcome without A's
participation. If A is in fact still running, its already-open handles keep
committing until they next restart. That is exactly the split brain this work
removes. The design therefore does **not** include break-glass in v1. If the
controller wants it later, it needs out-of-band physical fencing evidence
(power or storage destruction) as an explicit risk acceptance, recorded on the
witness as a distinct cause. It is never an ordinary command.

## Relation to non-owner intents

Nothing here changes the merge-back contract. B, while a non-owner, keeps local
intents in its separate intent store. After activation, A's late canonical
records are quarantined evidence, never replayed.

## Validation matrix additions

| Area | Negative | Positive control |
| --- | --- | --- |
| Inventory | Marker without record; record without marker; copied file; hard link; inode change | Matching marker and record admits continuation |
| Witness serialization | Two reserves for the same predecessor | Exactly one `Ok`, one `Conflict` |
| Lost reply | Reply lost after create | Read-back of the same body resolves to `Ok`; different body to `Conflict` |
| Rollback | Journal `ACTIVE(g)` restored after the witness reached `Retired(g)` | Unrestored journal at the witness head admits |
| Outage | Witness unreachable at startup | Running owner keeps committing through the outage |
| Ordering | Crash injected after each of the eight steps | Never two owners; identical operation resumes |
| Abandon | Reserve A->B, crash, abandon; then B tries to activate attempt `k` | B refused (no `RETIRED` outcome); A restarts at `Active(g)`; a new handoff is attempt `k+1` |
| Abandon after barrier | Abandon while A's journal is `RETIRING` | Refused locally; the identical retirement must complete |
| Outcome race | Retire and abandon of the same attempt race | Exactly one `OUTCOME`; the loser gets `Conflict` |
| Illegal sequence | `r<k+1>/RESERVED` without `ABANDONED` at `k`; `ACTIVE(g+1)` without `RETIRED` | Walk returns `UNKNOWN`; startup refuses |
| Pending recovery | Crash after step 6, and after step 7 | Anchored walk finds the predecessor outcome; activation resumes, or the journal completes `ACTIVE` |
| Wrong predecessor | `PENDING(g+1)` with a certificate that differs from the witness outcome, or names another destination | `UNKNOWN`; no activation |
| Retained fixture | A's already-open handle after an admitted retirement | B admitted exactly once; A refuses |

## Decisions needed before W2 and rollout

1. **Witness principal and storage.** Which existing service principal and
   storage location may hold witness records. No credential is minted by this
   design.
2. **Inventory path convention** for deployed hosts, and the backup exclusion
   rule that keeps the inventory and journals out of transfer sets.
3. **Break-glass.** Confirm that v1 has none.

Until those are decided, slices E1, W1, S1 and T1 are source-only and
private-fixture only. No lineage is enrolled and no production store changes.
