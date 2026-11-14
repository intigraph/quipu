# Conditional namespace removal

A disposable fixture begins with an empty effective registry, applies one
private declaration, then removes it from configuration. The old command
refuses `namespaces remove`; the candidate conditionally removes the retained
policy and returns the exact initial effective registry. Reusing its predecessor
receipt refuses. All database writes are confined to the disposable fixture.

Baseline executable: source `1f314a69d2343fa634f8bc06686e06ce30e5bc1c`.
Its namespace implementation files are byte-identical to main `bdb34e93`.
Candidate executable: source `415c564c44203dd26ac0cf7cae804a1317bf6a8d`; subsequent generated documentation
commits do not change its Rust input. This is candidate behavior, not deployment.

Download [the output log](output.log) and [timing](timing.log), then replay:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

This is a replayable terminal recording, requiring util-linux `scriptreplay`.
The output is committed as a UTF-8 transcript. To repeat with local
executables, set `NAMESPACE_BASELINE` and `NAMESPACE_CANDIDATE`, then run
`bash docs/probes/pr-demos/namespace-removal/run.sh`. The baseline probe uses
an earlier source executable with identical namespace implementation, rather
than a separately rebuilt exact-main binary.
