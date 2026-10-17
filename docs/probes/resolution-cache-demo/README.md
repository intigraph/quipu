# Shared label cache experiment

The original request path builds the complete label index on every resolve or
resolution-enabled episode. The candidate shares a transaction-checked index,
updates known committed label changes and rebuilds unknown changes outside the
request path. `cache_labels = false` keeps the original scan path.

This recording runs the same resolve input and one-node episode body against
separate copies of a private 268,000-label synthetic store. Each child starts in
a fresh process. OFF is the original scan behavior; ON warms the cache first.
The fixture has no embedding provider or attachments. Composed stores always
retain exact scans because read-only mounting does not certify immutable provider
lifetimes; the private external-provider mutation regression enforces this. This demonstrates the
label-scan cost, not production latency or provider/composition parity.

Run from the repository root:

```sh
bash docs/probes/resolution-cache-demo/run.sh
```

The ignored library experiment also measures absolute process RSS and eight held
index generations during writes. Allocator releases can make RSS decrease during
that phase; subtracting those readings does not isolate cache memory cost. Warm
and stale requests have different costs. Read the raw output rather than treating
this synthetic comparison as a live-service acceptance test.

The small output/timing pair is retained in this directory with the source. It
is a terminal recording, requiring download and a terminal replay tool:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Recorded source: `fa9b427dba991dc513c455ff8f1d22897364abdc`, based on
`739c5f9d608c55233e423d5a32ded3529b1e6cc5`. The runtime source bytes in the
recording match that source commit; the recording was made before committing it.
OFF/ON use the candidate's explicit fallback switch, which preserves the original
full-scan implementation, instead of building two different revisions.

Observed recording: resolve mean109.66 ->6.08ms, p95130.22 ->6.13ms after83.63ms
warm; one identical one-node episode133.70 ->67.48ms, both Created with zero
hints. Both resolve responses are identical empty candidates; the independent
correctness suite also covers positive labels, aliases and composed-store labels.
RSS after calls179276KiB OFF/174760KiB ON, with allocator-confounded held-generation
readings retained in the output. These are separate processes, not a pure cache
allocation measurement. No embedding-provider/live-production acceptance implied.

Recording: [output](output.log), [timing](timing.log); playback verified with
`scriptreplay`. Runtime tests:28 pass plus48 randomized write sequences; removing
the generation guard deliberately causes the external-writer assertion to fail,
then restoring the exact source makes the suite pass again.

Fallback configuration:

```toml
[resolution]
cache_labels = false
```

Follow-up safety correction keeps the recorded unattached path unchanged, and
refuses caching whenever any provider is attached. The external-provider mutation
control fails on the original candidate and passes with exact scan fallback.
The recording does not demonstrate acceleration for composed stores.
