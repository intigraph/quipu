# Atomic graph registration and legacy repair

An update can no longer commit graph facts and then return an error because
registration failed. Asserting batches register their graphs inside the existing
multi-graph savepoint. A later graph or registry failure rolls back every batch's
facts, registry changes and transaction rows. Existing registry classes, parents
and creation metadata remain unchanged; ROOT is not re-registered.

The first writable open after this upgrade performs an atomic, once-only repair
of local graphs holding current facts but missing a registry row. It considers
`main.facts` only; history-only graphs and empty interned names do not get rows.
A persistent marker prevents another scan on later opens. A failure to write the
marker rolls back the repaired rows too. Read-only and immutable opens do not
run the repair. Recovered rows have committed class, no parent, and the unknown
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
remained unregistered after baseline writable reopen; the candidate repairs it
and retains its fact. Neither probe contacts a live service.

Run `bash docs/probes/graph-registration/run-demo.sh` from the source root. The
recording pair `output.log` and `timing.log` can be downloaded and replayed with
`scriptreplay`. The baseline stdout is retained separately, without a fabricated
timed baseline recording. Five candidate controls cover endpoint refusal,
writable legacy repair, current-only/once-only preservation, marker rollback,
and rollback across multiple graphs.

A main merge is a deployment and will run the writable-open migration through
the normal delivery path. Name the revert of the actual merge and prior serving
SHA before landing; source fixtures are not production acceptance.
