# Actual writer-hold control

The isolated native HTTP control performs knot, stage and detached promotion,
waits for completion and checks the resulting fact. It requires positive
per-client knot/promotion hold counters and prints the completed promotion
acquisition count and seconds. This is a tiny private database, not production
latency, a deployed service or a load replay.

Base `1fc72ba6a57132d063bb38af9ded2521595fcbcb` has no writer-hold counters.
The candidate's exact executed source hashes are retained in
[source-sha256.txt](source-sha256.txt). The compiled test is copied before the
recording and its hash is retained alongside those source hashes.

Read [output.log](output.log). For timed playback download this directory and
run `scriptreplay --log-timing timing.log --log-out output.log`.
The small recording stays with its caption in this repository.
To reproduce from source, run
`just test --locked --features full --bin quipu-server writer_timing_native -- --nocapture`.

Expected result: one completed detached promotion has positive held seconds;
waits, pooled reads and post-unlock embedding are not part of the hold. Separate
focused controls cover the negative cases and registry cardinality. Natural
served acceptance remains pending normal deployment; no live write was induced.
