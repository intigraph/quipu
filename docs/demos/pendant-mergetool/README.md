# Interactive pendant conflict

[Terminal recording](demo.cast) · [Readable transcript](transcript.txt)

A disposable Git repository has two branches replacing the same functional
slot. The installed baseline rejects `git-mergetool` as an unknown command.
The candidate prints the standard decision layout, accepts the explicit input
`conflict:0 theirs`, and exits 0. The final immutable `pendant-check` verifies
one pack. No store or service is touched.

Baseline: clean installed CLI `1792a7a6771583876522cff6fa0dfa0f31e7b6ce`
(0.17.0). Candidate: source based on `c2e3a34979ba4d34144363e01b2350ab623231af`
with this PR's mergetool changes; executable SHA-256
`dba3884cf089ecd9dc577752f045a47ce4acb0e670372632c020eb89c0fffe01`.
The PR body identifies the reviewed head. The baseline is an installed older
revision, not a separately rebuilt PR parent; the same command is invoked on
both executables against the same pending fixture.

Reproduce with explicit baseline and candidate executable paths:

```bash
python3 docs/demos/pendant-mergetool/reproduce.py /path/to/baseline/quipu /path/to/candidate/quipu
```

The small asciicast v2 file preserves actual terminal output and timing from
`script`; its decoded output was checked against the transcript. Download and
replay it with an asciicast-compatible terminal player, such as
`asciinema play demo.cast`. Inline playback is not provided. Recording,
transcript and reproducer are retained with this repository's existing demos.
This demonstrates a source candidate, not an installed upgrade.
