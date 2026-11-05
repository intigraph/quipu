# Cooperative heavy read admission

The server can cap declared `owner-rows` queries before they take ordinary
read capacity. The default `heavy_read_operations = []` leaves scheduling
unchanged. Configuration lives in `[quipu.server]`; enabling it is a separate
deployment decision, not a consequence of landing the source.

When enabled with `heavy_read_operations = ["owner-rows"]`, at most
`heavy_read_permits` of those queries can occupy ordinary readers. The cap is
acquired before shared read admission. A waiting heavy query cannot reserve
a shared reader, leaving `N - K` ordinary readers available to other work.
The server validates `0 < K < N` against its actual usable ordinary pool after
point reservation. Unknown or duplicate operations, worker-process serving
and unavailable reserves refuse startup. No connection is added.

The operation uses the same finite header parser as request completion logs.
Absent, unknown or duplicate declarations do not match. This is cooperative
scheduling metadata, not authentication, query cost estimation or a privilege.
An undeclared expensive query still uses ordinary admission and can saturate
it. Declaring a listed operation cannot gain access to the point reserve: it
uses the capped ordinary path even when its text fits the point grammar.
The query evaluator, graph policy, row limits and text remain unchanged.

One original request deadline covers class waiting, shared waiting and query
execution. Cancellation while waiting returns capacity without starting
blocking work. Once dispatched, both permits move into the blocking closure;
canceling an HTTP future cannot release them while that work is alive. Active
read telemetry continues to describe actual blocking work. Waiting for the
new class cap is not a new telemetry lane or operation metric label.

Native controls exercise class-before-shared order, continued cheap progress,
the two deadline waits, cancellation before dispatch and permit ownership
after cancellation of a running query. Configuration controls use the actual
project TOML loader and validate defaults and unusable reserves.

These controls do not establish served fairness or production causality.
Production capture, configuration and activation remain separate gates.

## Finite private qualification

A single matched release/full build at `703e2ce8` was qualified on a retained
private copy with three actual owner-row query variants. Empty and K=1 arms
were interleaved for three rounds, each with four owner queries and a positive
ordinary cheap read after observed active load. Only the cap setting changed.
The experiment completed exactly 46 data requests in 140.375 seconds within
its fixed two-CPU quota, 8 GiB memory limit and 600-second deadline. All owned
servers were reaped and the reconstructible copy was removed afterward.

| Concurrent measure | Empty policy | K=1 policy |
|---|---:|---:|
| Cheap median, n=3 | 6.458 s | 0.378 s |
| Cheap nearest-rank p95, n=3 | 6.549 s | 0.389 s |
| Cheap HTTP 408 | 0/3 | 0/3 |
| Successful owner queries | 12/12 | 12/12 |
| Owner median, n=12 | 6.408 s | 3.128 s |
| Owner nearest-rank p95, n=12 | 8.143 s | 5.327 s |

The observed cheap p95 ratio was 16.83, exceeding the predeclared factor-five
target. With n=3, that p95 is simply the largest sample. This is a bounded
latency observation, not a statistical significance claim or a demonstrated
408 recovery. All six isolated owner requests and 24 concurrent owner requests
returned exact matching, nonempty rowsets; five cheap-only positive controls
per arm also passed. Each empty-arm active control saw four blocking reads;
each capped-arm control saw one before the cheap read.

The aggregate memory peak reached the 8 GiB hard limit over restoration and
all query phases together. It cannot identify query heap usage or be compared
with a peak from a different window. No process fault was observed. Raw data,
query texts and configuration remain private. [qualification.json](heavy-read-admission/qualification.json)
contains the scrubbed summary, including cold-inclusive isolated controls.
This qualification does not authorize installation or policy activation.
