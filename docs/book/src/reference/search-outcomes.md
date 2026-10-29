# Search outcome accounting

Search outcome accounting distinguishes successful empty retrieval from a
transport failure, timeout, refusal, or unknown response. It does not change
ranking, context admission, query execution, or response bodies.

## Logical calls and coverage

One outer operation owns one terminal event. Nested search/query operations share
that scope. Native MCP forwarding shares its scope with HTTP and waits for MCP
projection to finish; an HTTP 200 followed by a projection error is an error.
HTTP timeout/refusal overrides a late blocking worker result.

Instrumented surfaces are the native HTTP and MCP adapters, local CLI search and
query commands, and public tool functions used by embedded callers. Compatibility
node/fact/hybrid search and stored queries use the same accounting. Low-level
store/evaluator calls are outside this contract. External forwarding is HTTP
unless the native adapter establishes shared ownership. Client headers alone do
not establish an execution surface. Remote CLI coverage is not asserted.

| Outcome | Meaning |
| --- | --- |
| `hits` | Validated nonempty retrieval, including a boolean ASK answer |
| `empty` | Validated empty candidate set or query relation |
| `no_context` | Candidates existed; the validated admission contract rejected all |
| `error`, `timeout`, `refused` | Failed execution/delivery; never semantic zeros |
| `unknown` | Missing, malformed, contradictory, or interrupted evidence |

Candidate, admitted-context and result cardinalities are independently nullable.
Missing metadata is not zero. SPARQL `ASK false` is one boolean answer.

## Live metrics

`quipu_search_requests_total` counts terminal calls by bounded surface, operation
and outcome. `quipu_search_zero_results_total` counts only proven zero kinds:
`no_candidates`, `all_rejected`, and `empty_rows`.
`quipu_search_miss_reasons_total` uses a fixed reason vocabulary, with unknown
reasons grouped as `other` or `unknown`. Queries, graphs, caller identities and
raw reasons never become metric labels. Absent surfaces are not fabricated zero
series. Standalone processes have their own in-process counters; use journal
replay to collect their durable outcomes.

## Private capture

Set `QUIPU_SEARCH_JOURNAL_DIR` before process startup to an existing absolute
directory owned by the effective user with mode `0700`. Each process creates a
new random `search-<128-bit-hex>.jsonl` file, mode `0600`. Final symlinks and broad
permissions are refused. Appends are synchronous and synced before being counted
as written. This durability has a latency cost; measure it for your workload
before enabling capture. Capture failure does not fail the search request.

Events use schema `quipu-search-outcome/v1` and random logical-call IDs. Private
evidence includes bounded scrubbed query text, input/query fingerprints, selected
options, reported search scope, nullable revisions and bounded caller
declarations. Recognized credential forms are scrubbed, but this is best-effort
redaction, **not anonymization**. Keep journals private. Ordinary process logs
omit this evidence and caller provenance. Declared provenance is not authenticated
identity; missing fields remain unknown.

Records over 16 KiB are refused. A failed append preserves its possible partial
tail and stops further appends to that file. Capture metrics distinguish
`written`, `disabled`, `error` and `oversized`; the last-success timestamp is
absent until a durable append succeeds. Uninitialized capture has no health
series. Browser/Wasm capture is disabled. Process death before terminal capture
can lose a call; these journals do not establish complete crash-proof coverage.

No automatic deletion runs. Retain at least 30 days **and** all unresolved
evidence, including records not yet consumed or dispositioned by downstream RCA.
Never treat a replay cursor as an RCA disposition or deletion authorization.

## Bounded replay

Run the standard-library Python 3.11+ reader explicitly, or from your own reviewed
collector schedule:

```sh
python3 scripts/search-outcome-replay.py \
  --journal-dir /private/search-journal \
  --state /private/search-consumer/state.sqlite \
  --max-records 1000 --max-files 128
```

Both directories must already exist, be absolute, owned and private. State is
bound to the source directory identity. SQLite commits each validated event's
identity, fixed-label aggregate counts and source cursor atomically with full
synchronization. Metrics export reads bounded aggregate rows rather than
rescanning all historical events. Unknown state versions and nonempty legacy
state are refused; never reset state silently to get a healthy snapshot. Restarting,
re-reading, or encountering an identical duplicate does not recount it. A
conflicting identity or unknown/malformed record stops that file without
advancing past the evidence. Partial tails wait for completion. Truncation,
replacement and missing known files are visible source gaps. File selection
rotates between bounded runs; directory enumeration refuses coverage beyond
100,000 entries. Consumer source-file history also has a 100,000-row ceiling;
exhaustion is a visible refusal requiring explicit archival/migration planning.

The reader prints Prometheus text and never executes a captured query or contacts
a service. `quipu_search_journal_requests_total` is cumulative, deduplicated
consumer state, with a separate metric name from live process counters. Do not
sum these families: HTTP/MCP events can appear in both. Filter by surface when
combining standalone CLI/library journal counts with live server counts.
Resetting consumer state resets these counters and discards deduplication history.

Health gauges report the current scan's errors, partial tails, source gaps,
backlog bytes, processed records, inspected/uninspected files and scan timestamp.
Nonzero errors or source gaps return exit 2 while preserving health output;
unreadable state or source returns exit 1 without a fabricated healthy snapshot.
An absent/stale collector timestamp is unknown coverage. Publish output atomically
through your collector; preserve failure output for health rather than silently
retaining a previous healthy snapshot.

This reader supplies capture consumption, not the scheduled RCA programme,
repair-seed creation, dashboard installation, or production acceptance proof.
