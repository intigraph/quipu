# Default-off ingest pilot: legacy writer independence

This is a source-fixture transcript, not an installed-service demonstration.
No terminal recorder (`asciinema`) was available. The small transcript is kept
with the source so it remains available with the change.

Base: `93847e2d8ef30a66e0b29e26ea993da9687f6a51`.
On that base, `Router::knot` requires a mutable legacy writer even for a routed
graph. An adapter holding that writer behind a mutex must acquire it before
calling the method. This baseline is a source/API observation, not a measured
production stall.

The candidate adds `Router::routed_knot`, which requires no legacy writer
reference, and default-off `IngestPilot` admission. To reproduce its observed
behavior, use the candidate revision associated with this document:

```sh
just test --locked --no-default-features --features shacl --lib \
  graph_router -- --nocapture --test-threads=2
```

Watch the `routed_snapshot_cycles_complete_while_legacy_mutex_and_sqlite_writer_are_held`
test. It runs twenty snapshot replacements plus exact point reads while both
legacy locks remain held, and observes completion before releasing either lock.
A missing dispatch, wrong destination, leaked legacy write or blocked worker
fails the test. The other controls cover default-off preparation, invalid
selection/fences, multiple routes and invalid write pins. The existing router
controls still exercise ordinary ROOT writes and legacy bypass fencing.

Observed transcript, microseconds per complete write plus read:

```text
idle_p50_p95=(12438, 18118)
held_p50_p95=(10581, 13793)
legacy_changes_delta=0 legacy_data_version_delta=0

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured
```

These twenty-sample synthetic cohorts establish the held-writer control; they
are not a calibrated throughput comparison or production memory profile. The
last snapshot is replayed and still yields exactly one current value. The pilot
is disabled by default and this change adds no production server switch.
Startup integration, full consumer parity and a separately admitted migration
remain open.

## Lexical-index profile control

The original fence fixture had no FTS schema. A fresh CLI-initialized store
also contains `lexical_fts.graph_id`. The new regression was first run against
`a6dc4fa570975fe824b8d8e5cf22de453bf6f541` plus the test, before the classifier fix:

```text
PolicyDenied("graph owner: unclassified graph-bearing column lexical_fts.graph_id; review its fence class before routing")
test result: FAILED. 0 passed; 1 failed
```

After classifying that exact column as derived, all eleven fence controls pass.
The new control creates the index before and after fencing, proves routed fact
writes still refuse, checks unrelated writes still update the index, and
drop/backfill rebuilds it without releasing the fact fence. It also verifies
that no fence trigger is installed on the FTS virtual table. Reproduce with:

```sh
just test --locked --no-default-features --features shacl --lib \
  lexical_index_before_or_after_fencing -- --nocapture
```
