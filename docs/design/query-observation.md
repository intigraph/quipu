# Correlating query cost with request completion

An HTTP completion identifies its request, caller, duration and result shape.
The legacy query-start line contains only the first 300 characters of SPARQL.
Those records cannot reliably identify which complete query consumed store
capacity when several requests run concurrently.

Queries that reach the query admission path emit two additional JSON records:
`query_observation_start` and `query_observation_end`. They carry the
server-issued `request_id` and `server_process_id`; HTTP start and completion
records carry the same process identifier. Request numbers alone are not unique
across server restarts. Calls without the HTTP middleware record a null request
identifier rather than inventing a caller identity.

`query_sha256` hashes the complete UTF-8 SPARQL string. `input_sha256` hashes the
serialized parsed JSON input, including graph, temporal and protocol options.
It is not a hash of the original HTTP body or a certified snapshot identity.
Serialization across different versions need not produce the same digest.
The query and input byte counts are recorded, but their contents and request
headers are not copied into these new records. A digest is an identifier, not
anonymization of a guessable private query. These values are log fields, never
Prometheus labels.

The end record distinguishes:

- `dispatch_ms`: time from observation creation to blocking-worker entry;
- `store_wait_ms`: the observed interval around acquiring a store reader;
- `store_held_ms`: the observed interval after acquisition through query result
  preparation, including parsing, evaluation and response serialization.

These are monotonic elapsed milliseconds, not a CPU profile. Early exits may
include guard cleanup in the held interval. Null means the stage was not
observed; it must not be interpreted as zero. `worker_outcome` is `ok`, `error`,
`unfinished`, or `not_started`. An early exit without a final result is
`unfinished`, not an inferred successful evaluation.

The observation moves into the blocking closure. Its end record therefore
describes blocking work ending even when the HTTP future has already emitted a
499 or timed out. A closure dropped before dispatch reports `not_started` and
null stage timings. These records do not prove that a client received a response
or that cancellation stopped the worker. Requests rejected before this
instrumentation have no observation pair; absence alone is not execution proof.

This adds hashing and two bounded-field log records per admitted query. It does
not change query results, authentication, deadlines, pool capacity or metric
labels. Operational use requires the normal reviewed deployment, then a bounded
correlation check on the actual served build. Source tests do not establish
production cost or resolve an incident. Rollback is a normal revert of the merge
that introduced the records; previously emitted logs follow existing retention.
