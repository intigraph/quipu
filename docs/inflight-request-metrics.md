# In-flight requests and oldest-write age

Completed-response counters cannot reveal a request that never returns. Quipu
exports process-local gauges at `/metrics` independently of the store lock:

| Series | Meaning |
| --- | --- |
| `quipu_http_requests_in_flight{lane="write"}` | HTTP handler futures that have started but have not produced a response or been cancelled. |
| `quipu_http_oldest_write_age_seconds` | Monotonic age of the oldest such write handler; zero with no write handler. |
| `quipu_store_requests_in_flight{lane="write",phase="waiting"}` | HTTP blocking work and store admission. Waiting includes permit and blocking-pool queueing; active begins inside the executing blocking closure. |
| `quipu_store_oldest_write_age_seconds` | Age since the oldest outstanding write entered blocking work or admission, including both phases; zero when idle. |

The table shows example labels. HTTP lanes are `write`, `read`, and `other`;
store lanes are `write`, `read`, and `point-read`, with `waiting` or `active`
phases. No request ID, task, path or client is a metric label. All series have
explicit zeroes, including an idle newly started process. A failed scrape or a
stopped process is missing telemetry, not an observed zero.

HTTP writes reuse the existing method-aware authorization classification. For
example, GET `/rdf-graph-store` is a read and PUT is a write. HTTP read counts are
not a measure of read-pool occupancy: the separate store series reports that.
HTTP lifetime ends when response headers/body ownership are returned; a long
streaming response is not kept active until its stream closes.

A disconnected HTTP client may leave uncancellable blocking work running. Its
HTTP gauge drops, but its store guard moves with the blocking closure and
remains active until the work actually exits, including error or unwind. A
cancelled admission waiter is removed without starting work. Oldest age is
recomputed from the remaining writes after completion; a newer write cannot
reset an older stalled write's age. Generic blocking writes such as `/knot`
and promote receive the same lifetime guard even though they bypass admission.
Admitted work carries its existing token through the blocking adapter and is
not counted twice. Detached snapshot promotion and attestation thread hops
capture their own token before spawning; the token survives a returned response. The registry contains only current work; scraping it never
acquires the store connection or writer mutex.

These ages include queue time. They are **not writer-lock hold time**, evidence
of transaction progress, or proof of a wedge on their own. One request can be
represented in both HTTP and store scopes, and an episode can enter admission
more than once; do not add the two scopes together. Internal writers that never
enter HTTP blocking or store admission are outside these request metrics.

The private controlled stall-shape test holds 35 writes (one active, 34 waiting)
for 300 simulated seconds, checks count >=10 and increasing oldest age, then
checks cleanup and an idle fresh registry. Other tests exercise actual admission
and HTTP middleware cancellation, including generic blocking work and an
actual native stage/promote blocked behind the writer after its HTTP response. This is not
a reconstruction of October 1 telemetry: that incident predates these series.
Historical episode classification and alert thresholds require runtime evidence
after installation; an ordinary restart gap must not be called idle merely
because its scrape is absent. No live stall or production load replay is needed
to test the instrumentation.
