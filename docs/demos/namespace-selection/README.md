# Configured namespace fixture demonstration

The [terminal recording](output.log) and [timing data](timing.log) show the actual
candidate development generator on source `a73112f6`, using isolated stores.
They are terminal artifacts, not a video or an installed-service demonstration.
Replay with `scriptreplay --log-out output.log --log-timing timing.log`.
The small artifacts are retained with this source revision in Git.

The retained before-control transcript on the pre-fix seed binary reported:

```text
AssertionError: nonlegacy namespace leaked 528 legacy values
```

That before run also exposed an existing fixture provenance error. The corrected
legacy baseline removes only a repeated, changed description from a reused node;
it completes without changing the generator. The recording shows the candidate
passing custom namespaces through both explicit arguments and configuration,
matching corrected-legacy output and values, and refusing an invalid namespace
before removing a preexisting database. Its actual response is:

```text
PASS: explicit/configured custom namespace, corrected legacy identity, invalid namespace refusal
```

Build each generator revision with `just --command cargo build --bin
seed-fixtures --features dev-tools`. Using the corrected fixture assets, record
the old binary baseline with `scripts/test-seed-fixtures-namespace.py OLD_BINARY
--baseline-json baseline.json --record`, then run the same script against the
candidate binary with `--baseline-json baseline.json`. The public script copies
assets into fresh temporary directories; it does not access a running service.

The historical before transcript is not a second live run in this recording.
No existing service store namespace or configuration is migrated.
