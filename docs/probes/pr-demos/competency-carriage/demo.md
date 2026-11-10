# Competency carriage

[Download the terminal recording](demo.cast) and replay with
`asciinema play demo.cast`. This is an asciinema v2 terminal recording, not a
video. The runner did not have an asciinema player installed; event decoding
and the captured output were checked, but player playback was not verified.
The recording and reproducer are retained with the source under the existing
PR demo directory.

The same synthetic contract is offered to clean base `4af7162b` and clean
candidate `21eb23ba`. Base refuses the new option. Candidate carries seven
files, stages the contract, and refuses an altered command member with a hash
mismatch. A final no-contract control produces four byte-identical files on
both revisions. No server, model, signing identity or production data is used.

```sh
python3 demo.py /path/to/base/quipu /path/to/head/quipu /tmp/demo.cast
```

The CLI regression in `tests/cli_competency.rs` additionally alters commands
and recomputes the public seals: import still refuses because recompilation
differs. The recording shows the simpler unsealed-edit refusal. These are
local candidate controls; neither demonstrates deployment or command execution.
