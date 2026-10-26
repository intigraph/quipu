# Disk-store ownership and explicit adoption

A copied SQLite database preserves `store_id` and `log_epoch`. Serving two
independent copies under that identity can splice unrelated histories into a
replica. Normal open must not silently grant a copy ownership.

New disk stores create `<canonical-database-path>.ownership.json`, recording
version, stable host identity, canonical path, store ID and epoch at adoption.
Linux uses `/etc/machine-id`; macOS uses `IOPlatformUUID`. Missing or invalid
host identity fails closed. Other disk platforms are not supported by this
ownership provider; in-memory stores do not need persistent ownership.
The host identifier assumes the operator maintains unique machine identities;
this is a local placement guard, not protection against a privileged operator
forging the database, machine identity or ownership record.

Existing stores with a missing, malformed or mismatched record open for
read-only inspection without migrations. Writes fail at SQLite. A configured
open that also requests attachments refuses rather than mounting them against
an unowned writer. Replication session admission checks the binding on every
open, including read-only and immutable connection constructors. Copying the
sidecar does not help: its canonical path and host must still match.

## Explicit operations

```text
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

The ownership file's adopted epoch is audit information. Freeze/thaw can mint
later epochs without changing the host/path owner. This guard cannot detect a
privileged raw replacement of the same lineage at its already owned path;
restoration must use the explicit restore/adopt operation.

## Failure and rollout boundary

An epoch commit and filesystem rename are separate durability boundaries.
An adoption error may follow a committed epoch change. Inspect the database
and record before deciding what to do next; do not report success or blindly
retry. Failed restore destinations are retained for inspection, not deleted
across an uncertain completion.

Legacy disk stores require explicit adoption. Installing this source without
an admitted adoption blocks their writes and replication. Source approval is
not operator authorization to adopt a production store. Plan the placement,
active sessions, follower re-bootstrap and rollback before activation. Rolling
back code does not restore an old stream epoch or authorize cursor reuse.
Ownership files are local authority and must not be transported as a way to
authorize a reconstruction on another host/path.
