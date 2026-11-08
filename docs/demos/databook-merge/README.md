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
`6cb3379a0d1f6bbc3cda0e7e7d159b245c08ae685aedcdef2b3c149b06115494`.
Its build identity reports parent `87a319093d1111a4759bd6c15d8b62d5ba35e5cd`
with the DataBook source changes present (`git_dirty: true`); the recording
does not claim that parent alone implements the adapter.

Repeat with both executable paths:

```sh
python3 reproduce.py /path/to/older/quipu /path/to/candidate/quipu
```

The asciicast contains actual terminal output and timing (nine output events,
0.235 seconds). Download it for a compatible terminal player, for example
`asciinema play demo.cast`; this page does not promise inline playback. The
repeat script asserts the final document is exactly the selected version,
including its prose, and verifies the result through the immutable Git check.
