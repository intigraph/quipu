# Embedding model identity source demonstration

Trigger: publish identical entity text, replace the synthetic embedding provider,
then publish that same text again. The baseline on `b7068fdf` queued zero vectors.
A dimension change already caused search refusal; a same-dimension model change
silently searched old vectors. The candidate queues one replacement in both
cases, and its dimension-swap vector has the new dimension. A separate control
checks mixed model identities, partial rebuild, provider rollback and raw UNKNOWN
vectors.

The [baseline](baseline.txt) is retained actual stdout, with no timing recorded.
The candidate [output](output.log) and [timing](timing.log) form a replayable
terminal recording of private source fixtures. Download both and replay with:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Playback requires `scriptreplay`; this is not an inline video. To reproduce,
build the library test executable with `cargo test --features shacl --lib
--no-run`, export its path as `MODEL_IDENTITY_TEST_BIN`, and run
`bash docs/probes/model-identity/run-demo.sh`. The fixture uses synthetic vectors;
it does not load an ONNX model, contact a service, rebuild a live vector store or
prove installed behavior. Recording artifacts are retained here with the PR.
The candidate source revision and verification are recorded in the PR body.
