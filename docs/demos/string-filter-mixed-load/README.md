# String-filter mixed-load demo

The same request selector has zero string shapes at base `d798abfc` and ten in
the candidate. The replay also grades retained native benchmark reports: the
intact engine passes; disabling `string_pushdown::narrowed` rejects all seven
ratio-guarded narrowing classes. Q4's mixed p99 also exceeds its ceiling.

Run `python3 docs/demos/string-filter-mixed-load/replay.py` from a full clone.
This is a replay of actual private-fixture results, not a new timing trial.
`proof.json` retains the reports, binary hashes and fixture identity. Both
starting databases are byte-identical, integrity-checked SQLite snapshots with
2,458,611 triples. The engine source is `d798abfc`; its build reports a dirty
worktree because of the uncommitted harness change. The sabotage changes only
the narrowing function and is restored before publication.

The native intact command exits zero with 144 requests, nine samples per string
shape, zero errors and 39 reads overlapping writes. The same command with the
mutant exits one, with zero HTTP errors and 41 overlapping reads: the refusal
comes from the performance ratchet, not an unavailable server. Q4's ratio is
225.23x intact and 0.86x with narrowing disabled; its mutant mixed p99 is
11,124.501 ms against 8,000 ms. No production requests or imports are involved.

The large-fixture profile reports legacy-operation timing and memory as
context. Their original small-fixture limits remain enforced in the separate
standard mixed-load job. Full-suite and quality results are independent of this
recording. Production smoke and deployment remain outside this demonstration.

Recorded against candidate `b1642c7b0ddf5191861067a77b820dc31e518bbb`.
The recording-only follow-up leaves the harness unchanged. Read the
[transcript](transcript.txt), or download [output.log.gz](output.log.gz) and
[timing.log](timing.log), decompress the output, then replay with
`scriptreplay --log-timing timing.log --log-out output.log`.
