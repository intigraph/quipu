# Session capture structured provenance guidance

Trigger: the same Stop event asks for a session-capture episode. Before, the
solicitation names its client label and free-text source but no structured
provenance headers. After, it names all five fields, requires environment-derived
values, holds capture pending when fields are missing, and distinguishes
declarations from authentication. Existing authenticated-writer and credential
guidance remains.

Source base: `4cd9e48d`; candidate code: `0f3cb748` (plus this demo artifact).
The demo uses actual CLI binaries and isolated temporary hook state, with no
HTTP, production episode or installed-hook change. It demonstrates the emitted
instructions, not whether another agent obeys them or which caller caused a
provenance metric.

[Terminal recording](output.log), [timing](timing.log). Download both and run
`scriptreplay --log-out output.log --log-timing timing.log`; terminal playback
only, no inline-video claim. The private launcher path in recorder metadata is
redacted; command output is unchanged. These small assets are retained with the
source demo. Repeat with explicit binaries:

```sh
bash docs/probes/pr-demos/session-capture-provenance/run.sh BEFORE_BIN AFTER_BIN
```
