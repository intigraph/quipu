# Atomic SHACL document

This local synthetic demo submits a type and its required name as two chunks.
Watch the independent write stay refused, then the assembled document commit
both facts in one operation. The control adds a second name and is still refused.
This demonstrates the existing atomic `/knot` route, not a weaker write gate or
an installed deployment.

[Output recording](output.log) and [timing](timing.log) are a replayable terminal
recording, not a video. The output is also a readable transcript. Replay with:

```bash
scriptreplay --log-out output.log --log-timing timing.log
```

Reproduce with two locally built `quipu-shacl-conformance` binaries:

```bash
BEFORE_ADAPTER=/path/to/base-adapter \
AFTER_ADAPTER=/path/to/candidate-adapter python3 run.py
```

The base is `d854c0897597dfb6c9f0e42ba6ba55084274c41e`, adapter SHA-256
`952261134692847b47b0872337857edfe0974e0f4e0fdafa75231027d4d12af0`.
The candidate was built from the working change on that base, with adapter
source SHA-256 `fdd1ed3922011b1ecde06d709246990784fd27791cc4e45d32378dd241e04075`
and binary SHA-256 `33d81d2b4a38a179cc87a15ee791562acfbba444cbba685d2e0f30e66f710f85`.
Both used the dev profile with default features disabled and `dev-tools` enabled.
The candidate source is included in this change; its build was not a clean
release build. All writes use disposable in-memory stores. No service, credential
or production graph is involved.

The small output/timing files are retained here with this repeatable demo. The
full W3C corpus and private qualification logs are not bundled in the repository.
