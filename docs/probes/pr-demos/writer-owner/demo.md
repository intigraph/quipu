# Local owner CLI

[Download the terminal recording](demo.cast) and replay with
`asciinema play demo.cast`. This is an asciinema v2 terminal recording.
The runner lacked an asciinema player; event decoding and captured output
were checked, but player playback was not verified. The small recording and
reproducer remain in this repository's existing PR demo directory.

The same `graph writer-instance urn:demo:target` probe runs against isolated
local stores. Clean base `21eb23ba792065c4ffaa149eb76f19536db4a355`
rejects the command; clean candidate
`e8f00a2e4c77e0fc6b712ced441145d72ce051a6` returns the actual store and
graph-generation pins. Base runtime source is byte-identical to proposal base
`ce86ba4cc1b5bf0bcf98a67c2de8a6e1f075946b`; their differences are demo files.
Later candidate commits add generated documentation and this demonstration.

The candidate enrolls an Update-only grant for that observed instance, refuses
reuse of revision 1, and records revocation at revision 2. Malformed input is
refused without echoing its contents. The synthetic grant contains identifiers
and pins, never a bearer. Existing integration controls additionally verify
that the grant permits its intended scope and revocation denies it.

```sh
python3 demo.py /path/to/base/quipu /path/to/head/quipu /tmp/demo.cast
```

No server, production graph, credential enrollment or activation is involved.
The recording demonstrates local candidate behavior, not installed acceptance.
