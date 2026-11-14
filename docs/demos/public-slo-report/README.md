# Public daily SLO report

The recording compares the installed clean `cd86223b` CLI, which refuses
`report --public` before opening a store, with the candidate runtime source
committed as `f5f2a538`. The follow-up adds this recording and an optional display
in the fixture; it does not change runtime code.

The candidate reads a private local HTTP fixture with synthetic credentials and
statistics. Its displayed row comes from the actual CLI, including fresh random
client and nonstandard endpoint labels. The same run verifies eight authenticated
reads, no graph/config access, no private labels in output, missing-data nulls,
and partial-response, redirect and size-cap refusals. No fleet service is changed.

Replay after building the candidate with `cargo build --features remote --bin quipu`:

```bash
export QUIPU_DEMO_BASE=/path/to/pre-change/quipu
export QUIPU_DEMO_CANDIDATE=/path/to/candidate/quipu
bash docs/demos/public-slo-report/demo.sh
scriptreplay --log-timing docs/demos/public-slo-report/timing.txt --log-out docs/demos/public-slo-report/output.txt
```

`output.txt` and `timing.txt` are a terminal recording, not an inline video.
The baseline must be a CLI without `--public`; a different result stops the demo.
The synthetic two-day fixture is not a production benchmark or retention proof.
Separately, an authenticated candidate read returned all 30 UTC days from
2026-10-11 through 2026-11-09 in 5.8 seconds, with observations, latency and client
data on every day. Running retention was still 30d; 90d is a separate IaC change.
