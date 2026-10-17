# Episode resolution admission and batch deadlines

A long phase-zero episode now acquires a dedicated one-permit cap before
ordinary read admission. Waiting episodes therefore do not each reserve an
ordinary reader. The cap moves into the blocking closure, so canceling the
HTTP future cannot release capacity while resolution still runs. Both admission
waits use the original episode deadline. This limits episode concurrency; it
does not reserve service against unrelated ordinary-read floods.

Batch vector scans already checked their deadline every 4,096 rows. This change
also checks on entry, every 64 rows, every 16 query comparisons and around result
metadata fetches. Individual SQLite operations and embedding-provider calls
remain non-preemptible. No new timeout value is configured here.

## Demonstration

Target: isolated in-memory library fixtures, never a live service. Input: an
already-expired request deadline, one eight-dimensional query vector, and an
empty vector store. Baseline `21bd8e8e` with the identical regression test returns
`Ok([[]])`; candidate code `eed75a4e` refuses with `RequestTimeout`. All eleven
vector controls pass, including populated vectors, ties and temporal filtering.
The empty-store probe discriminates the entry check; it does not measure a cold
production scan or prove a wall-clock overrun bound.

The terminal recording is [output.log](output.log) with [timing.log](timing.log).
Download both and replay with:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Reproduce the private probe on candidate source:

```sh
cargo test --locked --lib vector::tests::expired_batch_scan_refuses_even_without_rows -- --exact
cargo test --locked --lib vector::tests
cargo test --locked --features server,onnx,shacl --bin quipu-server episode_cap_tests
```

Two server controls verify that cancellation retains a running episode's cap,
a queued episode leaves ordinary read capacity available, and a deadline while
waiting for read capacity releases the cap. Deliberately dropping the episode
permit before blocking makes the cancellation control fail.

## Limits before increasing the episode timeout

A timeout increase still needs installed-client identity and timeout checks,
margin below both client limits, and an authorized before/after probe of the
same 57-new-node input. Source inspection of a proxy's 30-second transport
setting is not an end-to-end wall-clock guarantee. These tests do not arm a
runtime value, prove production throughput, or authorize a deployment.
