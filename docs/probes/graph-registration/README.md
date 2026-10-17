# Atomic graph registration and legacy repair

An update can no longer commit graph facts and then return an error because
registration failed. Asserting batches register their graphs inside the existing
multi-graph savepoint. A later graph or registry failure rolls back every batch's
facts, registry changes and transaction rows. Existing registry classes, parents
and creation metadata remain unchanged; ROOT is not re-registered.

Opening a store never performs this repair, including writable opens. Run
`quipu graph-registry-repair --db PATH` (or add `--dry-run`) to count missing
local current graph registrations through a read-only connection. No marker or
registry rows are created by counting. Explicit `--execute` performs an atomic,
once-only repair of `main.facts`; history-only graphs and empty names do not get
rows. A persistent marker prevents another repair scan after successful explicit
execution. Marker failure rolls back recovered rows. Combining `--dry-run` and
`--execute` refuses. Recovered rows have committed class, no parent, and the unknown
creation-time sentinel `1970-01-01T00:00:00Z`; no historical creator is invented.

The marker is additive schema state and remains after a source revert. Pre-change
respace column classifiers do not recognize its `name` column and refuse that
maintenance operation; compatible maintenance tooling must classify it. No
reverse migration, deletion or automatic re-repair after later out-of-band
corruption is part of this change. Normal graph writers must use registration.

## Before and after

The baseline at `abb9e5a1` used a private endpoint fixture with a successful ROOT
write followed by forced registry failure: the endpoint returned an error but
left one target fact committed. The candidate leaves zero target facts and no
registry row. A legacy fixture with a current fact and missing registry row
remains unregistered after writable reopen in the revised candidate. Both default
and explicit dry-run CLI calls count one without repair; explicit execution
repairs it and retains its fact. Neither probe contacts a live service.

Run `bash docs/probes/graph-registration/run-demo.sh` from the source root. The
recording pair `output.log` and `timing.log` can be downloaded and replayed with
`scriptreplay`. The baseline stdout is retained separately, without a fabricated
timed baseline recording. Five candidate controls cover endpoint refusal,
non-repairing opens and explicit maintenance, current-only/once-only preservation, marker rollback,
and rollback across multiple graphs.

A main merge is a deployment, but does not execute this backfill. Name the revert
of the actual merge and prior serving SHA before landing. Live maintenance needs
its own operator authorization: first an authorized dry-run count, then a named
rollback and a separate execution GO. A source revert does not remove recovered
rows or the marker. Source fixtures are not production acceptance.
