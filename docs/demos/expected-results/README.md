# Deterministic conformance expectations

The same disposable test changes from known failure to pass. The old
`check_regression.py` gate accepts it; the new strict expectation gate refuses
it until an explicit author update. This uses real CLI exits and the source
checker, with no production graph, benchmark compilation or network access.

The old checker is unchanged from base `bc8d91fb`; retaining it for historical
fixture tests does not keep it as the CI admission gate. Full-suite timings and
source stamps remain run artifacts, separate from stable expectations.

Replay `output.log` with `timing.log` using `scriptreplay`. Repeat the probe:

```sh
python3 docs/demos/expected-results/probe.py
```
