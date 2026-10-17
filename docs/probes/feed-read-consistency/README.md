# Feed read consistency source demonstration

The baseline on `b7068fdf` returns transaction 2 with watermark 1 when a second
connection commits at a deterministic statement boundary. A missing transaction
table and a malformed transaction timestamp also become empty metadata instead
of errors. The same three falsifiers reject the baseline and pass the candidate:
watermark 2 covers transaction 2, and both query errors propagate.

Two additional controls use actual SQLite WAL connections. Read-only connections
created with the server pool's constructor exclude an actual uncommitted writer
savepoint, then see it after commit. A caller-owned read transaction keeps its
snapshot and remains open. Existing HTTP handler tests independently verify the
pool routing; the new library fixture does not claim to issue HTTP requests.

The [baseline transcript](baseline.txt) retains real stdout without timing.
The candidate [output](output.log) and [timing](timing.log) are a replayable
terminal recording of the private fixtures. Download both and use:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

To reproduce, build the library test executable with
`cargo test --features shacl --lib --no-run`, export its path as `FEED_TEST_BIN`,
and run `bash docs/probes/feed-read-consistency/run-demo.sh`.
Playback requires `scriptreplay`; no inline video is claimed. Artifacts stay in
this directory with the PR. The caption's source revision is recorded in its PR
body. No running service or production store was probed or changed.
