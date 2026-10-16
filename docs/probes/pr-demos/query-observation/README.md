# Query observation producer demo

The legacy log schema stops query text at 300 characters and cannot join the
complete query to HTTP completion. The candidate's private in-memory HTTP
control produces four records with a shared server process/request identity,
full query/input fingerprints, stage timings, and a real one-row result.

`records.json` retains the actual producer output. The baseline is source
inspection, not a paired production timing run. The control exercises the query
handler and logging middleware; it does not exercise authentication or a live
network. Timings describe this small control only.

Run the producer control with:

```sh
just test --features full --bin quipu-server http_query_observation_demo_has_a_real_result_positive_control -- --nocapture
```

Display the retained output without running any query:

```sh
python3 docs/probes/pr-demos/query-observation/demo.py
```

The terminal recording is [output.log](output.log), with [timing.log](timing.log)
for playback. It demonstrates log correlation; production cause, latency and
recovery remain unproven. See the [field contract](../../../design/query-observation.md).
