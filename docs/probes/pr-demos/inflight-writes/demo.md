# Pending-write visibility

Before this change, the served metrics at commit b7253fd2 had no in-flight or
oldest-write series. The candidate adds separate HTTP lifetime and store work
lifetime, including waiting and active phases. See the
[metric contract](../../../inflight-request-metrics.md).

The [recording](output.log) and [timing](timing.log) display values from the actual
candidate registry: 35 pending writes remain visible over 300 simulated seconds;
completion and a fresh-process control return to zero. This is a private
controlled fixture, not an actual production stall or a historical replay.

Repeat from the source checkout:

```bash
python3 docs/probes/pr-demos/inflight-writes/run.py
scriptreplay --log-timing docs/probes/pr-demos/inflight-writes/timing.log --log-out docs/probes/pr-demos/inflight-writes/output.log
```

The demo makes no service requests. The baseline version was observed separately;
no old incident is assigned a new gauge value. Other tests cover the real HTTP
middleware and admission path, including work that outlives HTTP cancellation.
