# Explicit truncation and lexical IRI ordering

The recording shows two retained failures from base `f22e46343b5501325a2e80e4bb23bc3bac7f575a` with the new regression tests applied, followed by actual fixture calls against implementation `013538f71ccf42dee8a16929a07b7b6364fbcc0f`.

Watch `ORDER BY ?s`: reverse-interned `z, a, m` becomes lexical `a, m, z`; descending reverses it. The HTTP handler then reports `X-Quipu-Truncated: false` below and exactly at a two-row fixture ceiling, and `true` when three rows are capped to two. The result body stays standard SPARQL JSON/XML or CSV/TSV. Separate automated controls cover RDF graph formats and the actual 10,000-row default boundary.

These are owned in-memory library/HTTP-handler fixtures, not an installed service or a deployment proof. The baseline segment replays genuine retained failures rather than rebuilding the base during playback. The default-ceiling and keyset controls are tests, not shown in this short recording. The public source retains this small output/timing pair with the PR.

[Recorded output](output.log) · [Timing](timing.log) · [Baseline transcript](before.txt)

Download both recording files into one directory and replay with util-linux:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The recording was replay-checked locally. To rerun the candidate portion from the repository root with the normal build prerequisites:

```sh
bash docs/probes/pr-demos/query-correctness/run.sh
```

The output filter hides compiler chatter; `pipefail` retains the actual test exit status. No credentials, network service or production store is required.
