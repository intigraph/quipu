# Community labels and stable identity

Replayable terminal recording on synthetic, disposable local databases. Base
`65db3e3c49f4e16b188b7c6b1e895f1428187cac` returns two unlabeled positional
communities; candidate source `297036ac798375af3ebb03c9a674730f588c46b4`
returns data-derived Alpha/Beta labels and retains Beta's identity after unrelated
Alpha edges are removed. Unchanged reruns match on both arms (positive control).

The candidate reports `git_dirty: true`: its source commit was clean, with three
private untracked planning files present; `build.rs` includes untracked files in
that flag. It is a source-built fixture candidate, not a deployed result. No
latency comparison is claimed between the installed base and debug candidate.

Download [output](output.log) and [timing](timing.log), then replay with util-linux:

```bash
scriptreplay --log-out output.log --log-timing timing.log
```

Replay was checked locally. No inline video player is claimed. The small recording
and caption are retained beside the repository's other PR demos.

Repeat the same input with [run-demo.sh](run-demo.sh):

```bash
bash run-demo.sh /path/to/base/quipu /path/to/candidate/quipu
```

Companion import has a separate real-Wasm round-trip test in
`scripts/test-community-companion.cjs`. Published explorer adoption and production
shape/performance acceptance are separate gates.
