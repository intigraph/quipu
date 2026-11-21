# Completed writer-lock holds

`/metrics` exposes `quipu_writer_lock_held_seconds_total` and
`quipu_writer_lock_acquisitions_total`, grouped by `endpoint` and normalized
`client`. These counters measure completed holds of the explicit writer guard,
including `/knot`, detached `/knot/promote`, and `/import/promote`. They exclude
acquisition wait and work after unlock. Multiple acquisitions count separately;
early errors and unwinds still release and record their acquired guards.

The timer starts immediately after acquisition, before attestation settlement
or write-budget rejection. Those gates retain their order and semantics. Metrics
aggregation happens after unlocking, so a scrape cannot extend the writer hold.
An HTTP disconnect does not end the timer of a worker that still owns the lock.
These are completed-hold counters, not an in-flight lock-age gauge.

Attribution reuses existing write provenance carried into blocking, deep-stack
and detached promotion work. It uses cooperative client declarations, not proof
of authenticated identity. Only the three fixed endpoint names above receive
separate labels; other write routes aggregate under `other`. Unattributed work
uses `internal` and `unattributed`. There are at most 256 ordinary series plus
one overflow series; arbitrary client labels cannot grow this registry forever.
No payload, raw URI, agent/session identity or credential is a metric label.

Reader interfaces, including writer-backed read fallback, do not contribute.
Direct uses of the raw writer mutex, such as replication and feed barriers, are
also outside these counters. This is explicit writer-path timing, not proof that
all occupancy of the physical mutex has been accounted for. Existing
`quipu_store_held_seconds_total` counters keep their separate meanings.

Compare rates of held seconds and completed acquisitions for the same client
and route. Their ratio is mean completed hold time. It is not a latency
percentile and cannot identify the holder of a past timeout on its own.
Production acceptance requires observing natural knot/promotion activity after
the normal deployment; no production promotion or load replay is required here.
Rollback is a normal source revert through the existing deploy lane.
