# Pendant history fixture demonstration

A topic branch once merged a fixture named `export.nt` without a pack manifest,
then renamed it to `payload.nq`. Strict historical replay rejects the repaired
branch before reaching its valid final head. Watch the same scratch-history
probe fail with `missing manifest` before and pass after.

The [terminal recording](output.log) and [timing](timing.log) compare the actual
CI script at base `65db3e3c49f4e16b188b7c6b1e895f1428187cac` with source
`deb2e38e`. Both use the candidate private CLI; its unflagged strict check retains
the baseline behavior, and only the new script requests historical mode.
This demonstrates source behavior against disposable Git fixtures, not deployment.
These small artifacts are retained with this document in Git.

Build a private CLI, set `DEMO_QUIPU` to its path, then reproduce from repository
root:

```sh
python3 docs/probes/pendant-history-demo/record.py "$DEMO_QUIPU"
scriptreplay --log-out docs/probes/pendant-history-demo/output.log \
  --log-timing docs/probes/pendant-history-demo/timing.log
```

Playback was checked with `scriptreplay`; download both files for terminal replay.
The output file is also the readable transcript; no inline video playback is claimed.

Historical mode skips only directories without a manifest in all four checked
snapshots. Every declared pack keeps hash, lineage, conflict and SHACL validation.
The final CI check remains strict, including manifest-less exports. The mixed
fixture/pack tests prove invalid historical pack merges still fail while valid
ones pass. Run `cargo test --features shacl --test git_qpack_history
--test git_qpack_merge` for these controls and the malformed final-head refusal.
