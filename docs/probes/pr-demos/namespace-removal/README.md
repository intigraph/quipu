# Conditional namespace removal CLI

The recording applies one private namespace in a disposable database, removes
its configuration, then attempts the conditional removal. The baseline rejects
the command. The candidate removes only that declaration and returns the exact
initial effective registry. Reusing the receipt refuses with a revision conflict.

Baseline: retained executable from source `1f314a69d2343fa634f8bc06686e06ce30e5bc1c`.
Candidate: this PR on base `027c424146e735697dae94f18f6b88944f99aca5`;
[exact Rust input hashes](source-sha256.txt) identify the compiled inputs.
The baseline is historical, rather than a separately compiled exact-base binary.
Current base source also lacks the removal CLI dispatch and catalogue entries.
This demonstrates local candidate behavior, not installation or registry approval.

Download the [raw recording](output.log) and [timing](timing.log), then run:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The raw recording is also a readable transcript. To reproduce against local
executables, set `NAMESPACE_BASELINE` and `NAMESPACE_CANDIDATE`, then run
`bash run.sh`. The fixture is temporary and removed after the run. The CLI
integration test additionally covers restoration, malformed requests, missing
arguments and stale receipt refusal without changing the registry.
