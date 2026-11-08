# DataBook merge: explicit choice, preserved document, immutable replay

[Recording](demo.cast) · [Transcript](transcript.txt) · [Repeat script](reproduce.py)

A disposable Git repository contains a declared DataBook with two competing
names under `sh:maxCount 1`. The candidate reports a graph-qualified decision,
accepts the explicit `theirs` choice through the shared mergetool, preserves the
unknown YAML field, human prose and unchanged shapes, then passes byte-for-byte
`databook-check` against the original immutable commits. Nothing is installed,
and no store, service, model or network is used.

The before executable is the older installed Quipu revision
`1792a7a6771583876522cff6fa0dfa0f31e7b6ce`, which refuses the mergetool command.
It is **not a rebuilt PR parent**. The candidate is an uninstalled copy of the
scoped-test executable, SHA-256
`ea3ec0c6e3cfe7b3be732efd14a19f9dfbe8536617b3d72db76167c870f42053`.
Its build identity reports `9a47af8f0c39e05e9c537152b99c5dac4a34ee87`
and `git_dirty: true`. All tracked Rust inputs, the manifest, lockfile and build
script matched that commit when the tested executable was copied. The recording
proves the copied artifact behavior; no installed acceptance is claimed.

Repeat with both executable paths:

```sh
python3 reproduce.py /path/to/older/quipu /path/to/candidate/quipu
```

The asciicast contains actual terminal output and timing (7 output events,
0.247 seconds). Download it for a compatible terminal player, for example
`asciinema play demo.cast`; this page does not promise inline playback. The
repeat script asserts the final document is exactly the selected version,
including its prose, and verifies the result through the immutable Git check.
