# Writer connection hold metrics

`quipu_writer_lock_held_seconds_total{endpoint,access}` measures time with the
actual server writer connection locked. The matching
`quipu_writer_lock_acquisitions_total` counts completed lock holds. These are
process-local counters; a restart resets them. Missing scrapes are not zeroes.

Timing starts after acquiring the fair mutex and ends immediately before
unlock. Queue wait, work before acquisition and deferred work after release
are excluded. Multiple holds in one request contribute separately, including
error returns and unwinding. Metrics aggregation happens after unlocking, so
its registry mutex does not extend writer occupancy. An unfinished hold is
not yet in these completed counters; in-flight request metrics serve a
separate purpose.

`endpoint` is the server's matched route template carried through the ordinary
blocking and deep-stack adapters. It is never a caller-supplied URI. Work
without that HTTP context is labelled `internal`. No task, entity, client or
request ID is added to this metric's labels.

The fixed `access` labels distinguish `writer`, `read-fallback`, and `barrier`.
A pooled reader contributes nothing to this writer family. A read or vector
search using the writer fallback contributes under `read-fallback`; it occupies
the same connection even though it does not mutate it. Feed synchronization
uses `barrier` and preserves its former lack of attestation settlement.

All server writer acquisitions go through the private handle field and its
owning guard. Deadline rejection still precedes attestation settlement in
`write_lock`; ordinary `lock` preserves its original settlement order. This
instrumentation does not change admission, transaction or deadline policy.
Existing store-time counters include other manually measured scopes and are
retained separately; do not add the two families together.

Private controls verify HTTP knot/promote route attribution across blocking
threads, several acquisitions, error/unwind cleanup, excluded queue wait and
post-unlock work, and pooled-reader versus fallback behaviour. They do not
constitute a production promote or reconstruct historical writer-held time.
Actual served promote acceptance remains a separate runtime check.
