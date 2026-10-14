# Governed memory in 60 seconds

[Download the recording](demo.cast) · [Read the transcript](transcript.txt)

This output-only asciinema recording uses **released Quipu 0.13.0**, revision
`ec3b707dc2976e579e634f276e752c7715a9ac99`, from
[the 0.13.0 release](https://github.com/scbrown/quipu/releases/tag/quipu-ai-v0.13.0).
Its recorded duration is 60.39 seconds. The data are illustrative fixtures
in this public repository, in an isolated local store; they are not historical
decisions or a demonstration against a running service.

Watch a valid decision enter, a second write get refused, and its corrected
version enter with a provenance source. `validate` displays the focus node,
property path and allowed tags; `knot` refuses the same bad data. An `ASK`
confirms the rejected decision stayed out, with the first decision as a
positive control. Finally, `unravel --valid-at` shows four facts before the
second decision and eight afterward. Those counts describe this fixture only.

## Play it

Download `demo.cast`, then:

```bash
asciinema play demo.cast
```

This is a replayable [asciicast v2](https://docs.asciinema.org/manual/asciicast/v2/)
file, not an inline GIF or video. Playback was checked with asciinema 2.4.0.
Only terminal output is recorded; no keyboard input or credentials are captured.

## Reproduce it

Install the release above. `quipu --version` must report 0.13.0, the revision
above, and `git_dirty: false`; the script checks the revision and clean flag.
From the repository root, copy the fixture into a fresh directory:

```bash
demo_dir=$(mktemp -d)
cp docs/demos/governed-memory-v0.13.0/*.ttl "$demo_dir/"
cp docs/demos/governed-memory-v0.13.0/run.sh "$demo_dir/"
(cd "$demo_dir" && bash run.sh)
```

The script refuses to reuse an existing `memory.db`. The fixed valid-time
dates make the before/after query repeatable. No server is started, no token
is needed, and no existing store is used.
