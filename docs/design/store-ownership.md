# Disk-store ownership and explicit adoption

A copied SQLite database preserves `store_id` and `log_epoch`. Serving two
independent copies under that identity can splice unrelated histories into a
replica. Normal open must not silently grant a copy ownership.

## Stage A: explicit inactive compatibility

Ordinary new and legacy stores retain their prior startup, writes and ordinary
source/relay behavior. `quipu store status --db <path>` reports `INACTIVE`.
No ownership record, enforcement marker or adoption epoch is automatically
created. Ordinary initial database creation still establishes its usual store
identity and log epoch; reopening does not mint an adoption epoch.
`Store::require_ownership()` refuses INACTIVE stores, including memory stores.
Legacy admission is not an ownership-authorized capability. A stream's
`Session::ownership_state()` exposes its captured admission state without
changing the existing signed wire contract.

Only explicit native adoption establishes `ENFORCED` placement. It persists a
sticky database marker and `<canonical-database-path>.ownership.json`, recording
version, stable host identity, canonical path, store ID and adoption epoch.
Linux uses `/etc/machine-id`; macOS uses `IOPlatformUUID` (not locally executed).
Missing or invalid native host identity refuses adoption, never falls back to
an invented identity. OCI host-side mount identity and browser physical-host
identity remain unavailable; their default operation is explicitly INACTIVE,
not a claim that those backends meet the ownership metric.

An enforced copy carries its sticky marker. Missing, malformed, partial or
mismatched marker/binding state opens only for read-only inspection without
migrations; configured attachments refuse. Source/relay admission refuses it.
Default constructors cannot downgrade it to INACTIVE. Deserializing enforced
bytes into memory also refuses before migration rather than discarding authority.
A stray ownership file without its marker is partial state, not legacy state.
Copying the sidecar does not help: its canonical path and host must still match.
The local marker is excluded from reconstruction shares; explicit restoration
establishes the destination's own enforcement and epoch after verification.

### Handles that are already open

Opening checks ownership once, but a writable handle opened earlier keeps its
connection. A later failed adoption (a sticky `pending` marker) or a lost or
invalid enforced binding must refuse that handle's writes just as a fresh open
does. Two layers do this:

- **Entrypoint checks.** `Store::ensure_ownership_writable()` re-runs the
  ownership check. Every fact and schema write entrypoint calls it before it
  writes anything, so the refusal carries the ownership error and no refusal
  audit row is written. This covers the transact pipeline and its callers,
  multi-graph batches, attested writes, the decision deck and nonces, labels,
  datasets, demotions, recorded graph creation, overlays (create, and every
  write arm including tombstones), forks, stored queries, freeze/thaw, the
  shape and ontology registry (load and remove), proposals (insert, accept,
  reject), replica apply and bootstrap, and share compose, review, promotion
  and pointer loads.
- **Commit fence.** Every writable file store also installs a SQLite commit
  hook. At each outer commit (autocommit statements and outermost `RELEASE`
  included), while the handle already holds the write lock, it repeats the
  check through a separate read-only connection and turns a refused commit
  into a rollback. The reader opens and reads the schema when the fence is
  installed, initializing its WAL access before any unlink. Each check starts
  a fresh read transaction: later adoption is visible even if the database
  path subsequently disappears, while genuinely legacy unlinked handles stay
  writable. A missing or failed reader refuses; no cached INACTIVE state can
  authorize a commit. This covers direct writers with no entrypoint check of
  their own, such as push subscriptions, attestation bindings, share-current
  pointers, pending share-query holds, quarantine purge, event consumers and
  event pruning. It also closes the window between an entrypoint check and the
  commit, so an adoption that fails concurrently cannot slip one write through.
  Its refusal surfaces as SQLite's commit-hook constraint error. SQLite leaves
  a refused outermost `RELEASE` marked as an open transaction, in which later
  writes would report success and then vanish. Every write scope therefore
  ends through one shared helper (`store::release`) that issues `ROLLBACK`
  immediately when its commit is refused and returns the error.

Derived index maintenance (lexical and substring indexes) and term interning
run inside the fenced write paths when a write drives them. Run on their own,
they add only dictionary or index rows that no fact references; on a fenced
handle their commits are refused by the commit fence as well. In-memory and
browser stores have no native placement and are INACTIVE, so they install no
fence.

This is a local placement guard. Host identifiers must be unique. A privileged
operator who deletes or forges both authority records can defeat it; this is
not hardware fencing. Stage A deliberately does not enforce ownership on all
legacy copies and does not complete the full ownership acceptance metric.

## Native qualification beyond Stage A

The native ownership fixture composes explicit adoption and restore with the
actual as-of cut producer, a fresh follower bootstrap and the signed tail. It
checks that the old follower's facts and cursor do not change across the new
epoch, that it cannot bootstrap over its existing state, and that a fresh
follower reaches the destination's scoped facts after a subsequent write.
In-place adoption ends the old source session with a signed `epoch_end`;
restoring to a different destination leaves the original source session valid.

These are disposable native-store controls, not production activation. The
path-copy and wrong-provider negatives do not establish an actual transfer
between two physical hosts. Native activation still needs its own placement,
session and follower disposition. Universal enforcement of unowned legacy
copies is not enabled by these tests. OCI mount/host authority and browser
physical-host ownership remain unavailable, rather than substitutes for the
original host-and-path contract.

## Explicit operations

```text
quipu store status --db /data/graph.db
quipu store adopt --db /data/graph.db
quipu store restore /backups/graph.db --db /data/restored.db
```

`adopt` requires an existing database. It mints a fresh epoch even at the same
host and path, commits it, then atomically replaces and syncs the ownership
file. The success output reports the new epoch. A pre-existing stream ends
with the existing signed `epoch_end`; its follower must bootstrap again and
cannot splice the new epoch into its old cursor.

`store restore` accepts a SQLite database, including its committed WAL state.
It uses SQLite `VACUUM INTO` for a consistent snapshot into a new destination,
then adopts it. It never overwrites an existing destination. This command is
separate from `quipu restore`, which reads a reconstruction pendant/text pack.
The source store ID is preserved; the destination epoch is new. Existing explicit
pendant/text-pack restore and offline term-space respace also adopt their new
placement after verification; ordinary file open never does. Explicit adopt
establishes lineage metadata only; a subsequent owned open performs ordinary
legacy migrations. It does not populate other excluded local authority rows in
a restored artifact.

The ownership file's adopted epoch must match the sticky marker's adoption
epoch. Freeze/thaw can mint later current log epochs without changing that
adoption record or the host/path owner. This guard cannot detect a
privileged raw replacement of the same lineage at its already owned path;
restoration must use the explicit restore/adopt operation.

## Failure and rollout boundary

An epoch commit and filesystem rename are separate durability boundaries.
An adoption error may follow a committed epoch change. Inspect the database
and record before deciding what to do next; do not report success or blindly
retry. The committed marker remains `pending` until the file is durable and
completion is recorded. A repeated adopt against a pending operation refuses
without minting another epoch; recovery needs an explicit operator disposition.
Failed restore destinations are retained for inspection, not deleted
across an uncertain completion.

Stage A deployment does not automatically enforce existing legacy stores.
Explicit adoption activates the sticky boundary and is an operator action;
source approval does not authorize adoption of any production store. Plan the placement,
active sessions, follower re-bootstrap and rollback before activation. Rolling
back code does not restore an old stream epoch or authorize cursor reuse.
Ownership files are local authority and must not be transported as a way to
authorize a reconstruction on another host/path.
