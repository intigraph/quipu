# Reader processes and aggregate memory sizing

Both settings are opt-in. Existing configurations keep four in-process readers
and legacy SQLite settings. Rollback sets `read_processes = 0`; removing
`memory_budget_mib` also restores legacy sizing after a normal service restart.

```toml
[quipu.server]
read_processes = 2
read_process_base_port = 3031
memory_budget_mib = 8192
```

The primary keeps its public listener and writer. It supervises separate copies
of the same executable on consecutive loopback ports, opening the same WAL
store through the existing SQLite read-only replica mode. Each child has one
connection and one forwarding slot. A blocked worker leaves other workers and
the primary writer available. Busy, unavailable, oversized or incomplete worker
responses return a refusal; requests are never retried or redirected to the
writer. Worker startup is checked against the actual child PID. Failure to start
any member refuses the pool. A failed worker is not automatically respawned;
restart the complete service through its normal lifecycle. Children stop when
the parent disappears, including after a parent crash.
The primary health endpoint reports a lost member as degraded (503), even while
the writer remains usable; healthy responses include the live reader count.

Only these bounded data routes use the process pool: `/query`, `/search`,
`/search_query`, `/unified_search`, `/hybrid_search`, `/search_nodes`,
`/search_facts`, `/search/nodes`, `/ask`, `/resolve`, `/graphs`, `/entity`,
`/fragments` and `/reconcile`. Other reads, streams, acquisitions and exports
remain on the primary; writes retain the existing primary authentication and
transaction path. Native MCP calls enter the same application and routing.
Workers never run primary writer jobs. Each worker uses two async runtime
threads. Replica metadata observations add one `/watermark` read per worker
every 15 seconds; account for these when comparing total request cost.

The private channel requires a random startup credential, never printed or
persisted to a config copy. Caller provenance and policy headers are forwarded;
the original socket peer is restored only through this authenticated channel.
Direct unauthenticated access to a worker refuses. The implementation currently
requires Linux, an on-disk local SQLite store, numeric HTTP bind addresses, and
no configured attachments, federation, external vector backend or point-reader
reservation. Unsupported combinations refuse before opening the store. It does
not support MCP stdio with reader processes.

There must be at least 64 MiB per connection and at most 32 reader processes.
Half the aggregate budget is reserved for models, query heaps, runtime and
buffers. A quarter sizes SQLite caches and another quarter sizes mmap, each
divided across long-lived serving connections: writer plus child readers, or writer plus
in-process pool when processes are disabled. Cache allocations are rounded down
to KiB. mmap is a ceiling and may be lower on a particular SQLite build. Maps
can share physical pages, so this accounting is conservative.

This setting sizes SQLite; it cannot independently guarantee RSS or prevent an
expensive evaluator allocation. Run the whole process group in one service
cgroup whose `MemoryMax` derives from the same MiB value. The companion IaC keeps
legacy behavior when its budget is null and, when configured, emits the exact
same integer into Quipu and the cgroup cap. It validates existing memory-floor
and recycler headroom rather than silently moving measured safety thresholds.
No CPU, cap, policy or production activation follows from source delivery.

Forwarded request bodies are limited to 4 MiB. Responses are limited to the
smaller of 64 MiB or one eighth of the aggregate budget divided by connection
count. Forwarding timeout uses the configured request budget, clamped to
1–120 seconds. The child still enforces its own request/evaluation limits.
Cancelled clients do not release an active forwarding slot prematurely.
Worker SQL observations are in the journal under their distinct PIDs; existing
primary ingress counters include forwarded reads. Internal per-process SQL
counters are not aggregated into the primary counters.
Measure aggregate cgroup memory and join request logs across every PID.

Run `just read-process-test SERVER_COPY CLI_COPY` on owned matching binaries for
scratch parity, real worker PIDs, write authorization, stopped-worker isolation,
worker loss and parent-death controls. Production acceptance needs a separately
authorized matched baseline/candidate window, completed-command parity, total
request/held cost across all processes, latency distribution, aggregate memory,
and a named pinned-source/config rollback. Source tests are not that acceptance.
