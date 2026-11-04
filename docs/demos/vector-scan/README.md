# SQLite vector scoring

This local, synthetic fixture compares the frozen scan from
`5d2fe613acd7cd2c172b23e3af8b2a5bddd612bf` with the candidate's actual SQLite
`vector_search`: 20,000 vectors of 384 dimensions and eight alternating runs
of each implementation. It checks identical result IDs, score bits and text,
including temporal and filtered selections, before reporting median timings.
The original algorithm is retained in the benchmark as an independent oracle.

The candidate borrows each SQLite BLOB, scores its little-endian coordinates
without allocating intermediate vectors, and computes the query norm once per
scan. It still scans every eligible vector. The measurement does not demonstrate
lower production latency, fewer logical reads, an index change or deployment.

Reproduce from the repository root with `bash docs/demos/vector-scan/demo.sh`.
The first invocation builds the optimized test binary. The benchmark is explicitly
ignored during ordinary tests; `just vector-scan benchmark` invokes it.

The output and timing files in this directory are a terminal recording, retained
in Git with this change. Download both files and replay with:

```sh
scriptreplay --log-out output.txt --log-timing timing.txt --maxdelay 1
```

The transcript contains the actual paired result; it is a synthetic fixture
comparison, not a live-service recording. The benchmark retains the base scan
from the exact revision above; the candidate is the source beside this caption.

The final-source qualification run measured a median of 14.801 ms before and
12.389 ms after (ratio 0.8370, 16.3% lower), with exact score bits and ranking.
Absolute times vary with host load; the recording is another invocation of the
same fixture, not a substitute production percentile or byte-I/O measurement.

The retained recording measured 13.559 ms before and 11.227 ms after (ratio
0.8280), with the same exact-result controls. It includes the actual build wait;
the replay command caps that wait for viewing without changing the stored timing.
