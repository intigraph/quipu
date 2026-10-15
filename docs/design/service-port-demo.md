# SERVICE fixture port isolation

When two consecutive ephemeral-port reservations return the same port, the old
runner starts both fixture stores at the same address. Its health check can accept
the first server for the second process even though that process failed to bind.
The resulting SERVICE2 rows contain Alan/Alan and Bob/Bob instead of the expected
book title and unbound value.

The [terminal recording](assets/service-port-demo.cast) and
[actual transcript](assets/service-port-demo.txt) show the same forced duplicate
allocation before and after the fix, using real local fixture servers and exact
W3C result comparison. Download the cast and replay with `asciinema play`; inline
playback is not promised. These small artifacts stay with this source change.

Base: `0f23ef3be3cc6ed32d62dacd4cf7c0986fc803bb`. The candidate is the harness in
this commit. Both runs use unmodified Quipu CLI/server 0.13.0, clean revision
`f5cd1bb38381696bc57b265380339607fb94f4e9`, and W3C suite revision
`369a90d1a60c021b746df2e411da0ff36258a758`.

Repeat from a full source checkout with the pinned suite and explicit binaries:

```bash
python3 docs/design/assets/service-port-repro.py \
  "$PWD" /path/to/rdf-tests/sparql/sparql11 \
  /path/to/quipu /path/to/quipu-server
```

The reproducer asserts the baseline failure and candidate pass. Only port
allocation is forced; the SERVICE requests, fixture stores and result comparison
are real. Twenty normal SERVICE2 runs also passed, and all seven pinned cases
retained six passes plus the existing unsupported variable-endpoint policy case.
The historical CI mismatch is identical, but its port collision is **INFERRED**:
the retained failed artifact does not contain its listening addresses.

The fix allocates distinct remote ports with bounded retries and waits for each
child's successful-bind message before accepting health. Failed startups are
reaped. It preserves the existing conformance assertions and regression gate.
This demonstrates the candidate harness; it does not claim a deployed service
change or eliminate arbitrary external process failures.
