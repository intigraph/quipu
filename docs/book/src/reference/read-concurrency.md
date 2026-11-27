# Read concurrency: read pool, read processes and replicas

Quipu can serve reads three ways. They solve different problems, so pick by
the problem, not by the name.

| Topology | What it is | Use it for | Default |
|---|---|---|---|
| **In-process read pool** | N read-only SQLite connections inside the server process, beside the one writer | Concurrent reads. This is "multi-read" on every server. | **On**, `read_pool_size = 4` |
| **Read processes** | Separate read-only worker processes under one memory budget; the primary forwards reads to them | Memory isolation of read work from the writer process | **Off**, `read_processes = 0` |
| **Replicas / relays** | Separate follower servers that apply the primary's change stream | Reads on another host, failover, offline crew hosts | Off (configured per follower) |

## In-process read pool (the default multi-read)

Reads take one of `read_pool_size` read-only connections; writes keep the
single FIFO writer connection. SQLite WAL lets those readers run alongside the
writer. Requests wait (in admission) for a free reader until their budget runs
out. See [REST API: Read Concurrency](rest-api.md#read-concurrency) for the
measurement behind the default.

Size it to cores and to the read mix: each reader is one connection with its
own page cache, and concurrent scans contend for memory bandwidth. Watch
`quipu_store_wait_seconds_total` on `/metrics`: rising wait means readers are
queueing. `read_pool_size = 0` makes every read serialise behind the writer
(the rollback).

### Cooperative heavy reads

`heavy_read_operations = ["<label>", ...]` with `heavy_read_permits = K` caps
how many requests that declare one of those operations (`X-Quipu-Operation`)
may hold an ordinary reader at once, so a burst of expensive reads cannot take
every reader. A client names its own operations; the deployment lists the ones
it schedules as heavy. Quipu attaches no meaning to any particular label. Each
must be a well-formed declared label (`[a-z][a-z0-9._-]{0,63}`), listed once.
It is scheduling, not authorization, and it does not classify undeclared
queries. `K` must be positive and less than the ordinary reader count (after
`reserve_point_reads`). With `K = 1`, listed requests queue behind each other;
raising `K` lets more run but leaves fewer readers for everything else, so
change it as a measured step.

## Read processes (opt-in isolation)

`read_processes = N` starts N read-only worker processes on loopback ports
from `read_process_base_port`. They are sized from **one** aggregate
`memory_budget_mib` that covers the primary and every worker, and the service
manager should enforce the same total (for example systemd `MemoryMax`).
Quipu refuses `read_processes > 0` without `memory_budget_mib`.

What changes when workers are on, and why it is not a free capacity upgrade:

- The primary forwards **every read endpoint** (`/query`, `/search`,
  `/search_query`, `/unified_search`, `/hybrid_search`, …) to the workers. It
  no longer answers them from its own read pool.
- Each worker takes **one forwarded ordinary request at a time**. With heavy
  reads enabled, worker 0 also carries the single service-wide owner-rows
  executor (`K` is never multiplied by the worker count).
- A read that finds no free worker slot is refused at once with
  `all read processes are busy` (HTTP 503). It does not queue, and it does not
  fall back to the primary.

So concurrent read capacity with workers is roughly N ordinary + K heavy
requests, and overflow becomes immediate 503s instead of waiting. One worker
is **less** concurrent than the default in-process pool of 4. Size N to your
measured peak read concurrency, and size `memory_budget_mib` to cover every
connection (at least 64 MiB per connection; Quipu validates this).

Verify workers are live:

- startup stderr: `read process pool: N separate read-only workers; total memory budget …`;
- `GET /health` returns `x-quipu-read-processes-alive: N`, or 503
  `{"status":"degraded","reason":"read_process_unavailable"}` when a worker is missing;
- the listening loopback ports `read_process_base_port … +N-1`.

Rollback: `read_processes = 0` and restart. The in-process pool resumes.

## Request budgets

Two budgets bound a read; both answer HTTP 408 when they run out:

- `[quipu.search] request_timeout_ms` (default 0 = off): one wall-clock budget
  for the whole request, stamped at admission, so queue time counts. A request
  that spends it all waiting reports
  `request deadline exceeded: budget …ms (elapsed …ms), including queue and execution time`.
- `[quipu.search] query_timeout_ms` (default 30000): the SPARQL evaluation
  deadline, started when evaluation begins. Overruns report
  `query timeout: exceeded …ms (ran …ms) — narrow the query …`.

The first message points at queueing (capacity or other callers), the second
at the query itself.

## Early read shedding

`read_admission_wait_ms` (default `0` = off) bounds how long a read waits for
an ordinary reader. Off, a queued read waits until its request budget ends and
then answers 408 with no retry hint. On, a read that is not admitted within
`min(read_admission_wait_ms, remaining request budget)` is refused **before it
runs** with HTTP 429, a `Retry-After` header, and a JSON body:

```json
{"code": "read_not_started", "read_started": false, "class": "normal",
 "reason": "wait_exceeded", "waited_ms": 1500, "retry_after_s": 2}
```

A shed read never touched the store, so it is always safe to retry after the
indicated delay. A deadline that expires after execution has started still
answers 408.

Callers that are not latency-critical (status displays, dashboards, pollers)
send `X-Quipu-Priority: background`. A background read never queues: it runs
only while more than `read_background_reserve` permits are free, otherwise it
is refused at once with 429. The header can only lower a request's priority;
any other value, or a duplicated header, is treated as `normal`.

`Retry-After` is estimated as the time for the reads queued ahead to drain:
the measured mean read service time, times the waiting reads plus one, divided
by the pool's capacity (reads running plus permits free), rounded up and
clamped to 1–30 seconds. Shed counts appear on `/metrics` as
`quipu_read_shed_total{reason}`.

With read processes on and shedding armed, `all read processes are busy` is a
429 with `Retry-After` (reason `workers_busy`) instead of a bare 503.
Transport, identity and response-budget failures of a worker stay 503.

Rollback: set `read_admission_wait_ms = 0` and restart.

## Identical overlapping queries

`[quipu.server] query_singleflight = true` opts into per-process sharing of
overlapping identical local `/query` reads. It defaults to false. Complete
input, scope, representation, authority and a live database generation fence
separate flights; completed results are never cached. Flights, followers and
shared response bytes are bounded, and each follower keeps its own wait budget.
Remote/non-deterministic queries use the ordinary path. Mounted and frozen-pack
stores may share reads only when every source generation and evaluation snapshot
is proven. An unavailable source, changed mount registry or replaced file bypasses
sharing. Startup reports `ACTIVE` only after the probes succeed.
The source design document `docs/design/query-singleflight.md` describes the
qualification boundaries.

## Replicas and relays

A follower server (`[quipu.server.replica]`, built by `quipu follow`) serves
reads from its own copy, refuses writes (naming `primary_url`), and answers
`GET /replica/ready` for failover checks. A relay (`[quipu.server.relay]`)
follows an upstream and serves its own children. These add read capacity on
**another host** rather than inside one server; see
[REST API: `GET /replica/ready`](rest-api.md#get-replicaready) and
[change feed](../architecture/change-feed.md).

## Every knob

| Key (`[quipu.server]` unless noted) | Default | Meaning |
|---|---|---|
| `read_pool_size` | `4` | In-process read-only connections; `0` serialises reads behind the writer |
| `reserve_point_reads` | `false` | Reserve one reader for bounded ground point-ASK requests (needs at least 2 readers) |
| `query_singleflight` | `false` | Share only overlapping identical local reads; no completed-result cache |
| `heavy_read_operations` | `[]` | Cooperative cap; currently accepts only `["owner-rows"]` |
| `heavy_read_permits` | `1` | Max concurrent declared heavy reads; `0 < K < ordinary readers` |
| `read_admission_wait_ms` | `0` | Shed reads not admitted within this many ms with 429 + `Retry-After`; `0` = off |
| `read_background_reserve` | `1` | Permits a `background` read leaves free for normal reads |
| `memory_budget_mib` | unset | One aggregate budget for primary and workers; required for workers |
| `read_processes` | `0` | Read-only worker processes; `0` keeps in-process serving |
| `read_process_base_port` | `3031` | First loopback port for workers |
| `[quipu.search] request_timeout_ms` | `0` | Whole-request budget from admission (queue + execution) |
| `[quipu.search] query_timeout_ms` | `30000` | SPARQL evaluation deadline |
| `replica` / `relay` | unset | Follower / relay serving (separate server) |

## Choosing

- Reads queue (`quipu_store_wait_seconds_total` rising, 408s with
  `request deadline exceeded`): look at the callers first, then raise
  `read_pool_size` within cores and memory.
- Owner-row reads queue behind each other: consider `heavy_read_permits`.
- Read work must not share memory with the writer: read processes, sized to
  peak concurrency, under one `memory_budget_mib`.
- Reads needed on another host or during primary downtime: a replica.
