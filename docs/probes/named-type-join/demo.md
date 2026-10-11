# Composed query demonstration

A current named-graph owner/type/status query runs against the same synthetic
fixture with one unrelated read-only archive, four concurrent readers and three
rounds per model mode. Watch the 24 deadline failures become 24 completed reads.
The candidate verifies the mounted count and an independent ordered 100-subject
oracle. Increasing the deadline is outside this change.

Base source: 45bea073064d4eb6ba61d8a0432d98a575d41570 plus diagnostic-only server-acquisition synchronization.
Candidate source: d2ff4a6f6cb6742d0a115ca43ce4371c46c73de8. Later recording-only commits preserve these runtime blobs.
Author-built binaries were compiled from those source blobs before committing;
their build metadata reports the prior base plus a dirty tree. This records
private behavior; served acceptance follows reviewed deployment.

[Download recording](composed-demo.log) and [timing](composed-demo.timing).
CRLF was normalized to LF and event byte counts adjusted; output content and
recorded delays are preserved. Playback was verified with util-linux scriptreplay; downloading both files and
a compatible terminal replay tool is required. Inline playback is not claimed.

~~~sh
scriptreplay --log-out composed-demo.log --log-timing composed-demo.timing
~~~

The matching disposable database, query and independent subject file remain
available to authorized reviewers with the source packet. The portable probe is:

~~~sh
QUIPU_PERF_DB=/path/to/disposable.db \
QUIPU_PERF_QUERY=/path/to/query.sparql \
QUIPU_PERF_EXPECTED_ATTACHMENTS=1 \
QUIPU_PERF_EXPECTED_SUBJECTS=/path/to/expected-subjects.txt \
just test --release --no-default-features --lib \
  external_query_fixture_parallel_perf -- --ignored --nocapture
~~~

Retained binary SHA256:

- Before: 9485a590c402c35bb4048ba8bf9f9ce7493a416b3b020e8c4d0f26f1a75e5b4d
- After: ff95b3741f3c48f501948b11559ad639ea8d7890b6fa89caed3e886e09532109

The small sanitized recording and caption use this existing probe directory.
They remain in repository history for review; no external artifact host or upload
was provisioned. [Readable before](composed-before.log) and
[after](composed-after.log) transcripts retain the measured samples.
