# Typed LIMIT leaf: bounded native work

The trigger is `SELECT ?focus WHERE { ?focus a ?type } LIMIT 1` over
10,000 synthetic explicit type facts. Before: 120,101 SQLite VM instructions
before returning one real focus. After: 52 instructions with one real focus.
The measurement uses a development, no-default-features native test build;
it is not installed-service latency or proof of an incident’s memory cause.

The baseline is 360f8f49 with the new regression test injected. The recording
shows that retained, actual failing baseline transcript, followed by a fresh
candidate run and semantic controls. It does not replay a baseline executable.
The demonstrated source is this change’s test and eligibility implementation.

[Transcript](output.log) and [timing](timing.log) are an output-only terminal
recording. Download both, then use
`scriptreplay --log-out output.log --log-timing timing.log`.
No inline player is claimed.

Compile with `cargo test --no-default-features --lib --no-run`, set
`QUIPU_TEST_BIN` to the reported native test executable, then run
`python3 docs/probes/pr-demos/typed-limit/run.py`. Semantic controls cover
physical duplicates, numeric identity, projected bags, attached aliases, default
graph unions, missing graphs, constant-class subclass expansion and the existing
negative eligibility gates. A bounded-work test must fail on the base.

The small transcript and timing files are retained in this repository for review.
Production recovery remains a separate gate: two normal comments below five
seconds, after the normal review, merge and deployment paths.
