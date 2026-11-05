# Write refusal accounting

Quipu counts refused write attempts separately from successful writes and
validation results. One rejected attempt with several violations contributes
one count. Successful writes and hypothetical `speculate` refusals contribute
none. Existing admission decisions and error responses are unchanged.

`GET /metrics` exposes
`quipu_write_refusals_total{reason,shape,graph,client}`. Reasons are `shacl`,
`policy`, `authority`, `owl`, `placement`, `cannot_verify`,
`operational_failure`, `conflict`, or `other`. A refusal does not prove that the
submitted facts are false: `cannot_verify` and operational failures explicitly
preserve that distinction.

Shape identifiers come from structured validation feedback. Missing metadata
is `unknown`, blank-node shapes are `anonymous`, and several distinct shapes
are `multiple`. Graph is the destination graph IRI; client uses the existing
normalized request attribution, falling back to `unattributed`. Label values
are bounded and escaped. At most 1024 series are retained; overflow preserves
counts by reason while replacing shape, graph and client with `other`.

`GET /stats` and the MCP `quipu_stats` tool include `write_refusals` with
`total`, `by_reason`, `series`, `series_limit`, `event_record_failures`,
`counter_scope`, `reset_on_restart`, and `since_unix_seconds`. This process
snapshot is refreshed independently of graph-count caching. `/metrics` reads
in-memory counters and does not scan the event log or acquire the writer.

## Restart and history

Counters are process-local and reset on restart; they are not durable lifetime
counts. Prometheus can account for observed resets using, for example:

```promql
sum by (reason) (increase(quipu_write_refusals_total[30d]))
```

This is an estimate over retained samples, not an audit count. Missing scrape
history or a missing series is unknown, not evidence of zero refusals.

Each real refusal also uses the existing durable `write.refused` event after
rollback. Query `GET /events?types=write.refused` for retained audit history;
event retention can remove old records. Events add `reason_code`, `shape`
(null when unavailable), and normalized `client` to the existing payload.
Refused datum bodies are never recorded. Event recording remains best effort
so an audit failure cannot mask the original refusal. Such failures increment
`quipu_write_refusal_event_failures_total` and `event_record_failures` in stats;
the refusal counter still advances.
