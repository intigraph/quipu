# Mixed load performance ratchet

`scripts/quipu-load-test.py` drives a running Quipu server through a deterministic
mix of bounded SPARQL, full-graph SPARQL, inferred `rdf:type` SPARQL,
precomputed-vector search, and episode writes. The fixture loads a subclass
ontology and writes instances of that subclass, so both the query-time RDFS
expansion and default reactive OWL write path are exercised. It steps through
concurrency 1, 4, and 8 and records p50/p95/p99 latency,
throughput, HTTP error classes, and server peak RSS.

The CI job starts an empty ephemeral server and publishes both the JSON report and
server log. No embedding model or external service is required: `/search` receives
a fixed 384-element vector.

Run it locally after starting `quipu-server`:

```sh
scripts/quipu-load-test.py \
  --url http://127.0.0.1:3030 \
  --baseline benchmark/load-test-baseline.json \
  --output load-test-report.json
```

The checked-in limits are deliberately runner-tolerant absolute guardrails. Tighten
them only from repeated CI observations; do not loosen them to clear one slow run.
The `baseline` object preserves the fixed-corpus result from `aegis-bzofv` as the
first historical point, while `limits` controls the CI verdict.

`read_progress_during_writes` counts successful read requests whose execution
overlapped an episode request. A zero fails the ratchet: it means the run did not
demonstrate that WAL readers progress while Quipu's single fair writer is occupied.
HTTP 408 is reported separately from 502, client timeout, transport failure, and
other status codes. A 408 means the query exhausted its own budget; it is not by
itself evidence that the writer lock or server is wedged.

## String-filter mixed load

The required string-filter CI job also runs this ratchet on its deterministic
150,000-entity query fixture. `--string-filter-budget
benchmark/query-perf-budget.json` interleaves the exact shared Q1–Q9/C0 query
shapes with the existing six operations. The CI command requests 48 operations
at each concurrency level, giving nine samples per string shape overall. Literals,
subject and predicate IRIs, and graph names exercise CONTAINS, STRSTARTS, REGEX
and LCASE. Q7 remains explicitly known-slow. Semantic search selects ROOT
explicitly on this multi-graph fixture; named-graph backfill is not enabled.

Every string response must contain valid, nonempty bindings. Missing classes,
fewer than three samples, HTTP/semantic failures, or excessive mixed p99 fail.
Mixed p99 permits up to eight times the existing uncontended p95 ceiling for
queueing at concurrency eight; this does not change the uncontended budgets.
After the mixed phase, the same process runs the original C0-relative and
pushed/defeated differential gate with the existing budgets. That in-run ratio
must reject disabled narrowing, even when the mixed absolute ceilings pass.
The JSON report retains both the concurrent samples and the differential result.

This mode requires the large query fixture and `--baseline`; it refuses a
configuration that could print failed differential results and exit successfully.
The original small-fixture job remains a separate six-operation control. All
fixtures and sabotage runs use private stores. Production replay and post-deploy
smoke require their own runtime admission.

The large-fixture profile grades string latency, differential correctness,
relative speedup, zero errors across the complete mix, and read progress during
writes. It reports throughput, RSS growth and the six legacy operation timings
as context. Their small-fixture limits remain unchanged and enforced by the
original mixed-load job; applying those limits to a different corpus would not
be a comparison with that baseline.
