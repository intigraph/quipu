# Agent reference coverage

Run from the repository root:

```sh
bash docs/demos/agent-reference/probe.sh
```

The same `search --limit` man-page entry on base
`d707ec62fe105f3296143321e681aec8daa46f28` and the generated candidate changes
from a blank description to its catalogue text. The candidate then accepts the
four complete agent-reference sources and refuses each missing member in a
private copied fixture. It never modifies the actual sources or publishes a
pendant. Native index-pointer and receiver-query controls are separate tests;
this demonstration does not establish release availability or deployment.

The raw terminal output and timing can be replayed with util-linux `scriptreplay`:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

## Native five-document round trip

Use matching native Quipu CLI/server siblings, Bobbin and its compatible CPU
runtime. The probe explicitly selects `BOBBIN_GPU=0` for its private child process
and closes its stdin, so recording cannot prompt or provision a GPU. The probe uses the producer's actual index configuration, copies only
README and the four actual reference sources into a private fixture, indexes
with Bobbin, then verifies document pointers and exact paths in both the index
and an imported internal share. No publication policy, live graph or release
credential is needed. This is an internal fixture, not outward publication or
full-repository indexing acceptance.

```sh
python3 docs/demos/agent-reference/index-probe.py \
  "$PWD" /path/to/quipu /path/to/bobbin
```

For the recorded qualification, Bobbin 0.31.0's release-matched CPU runtime
1.24.1 was selected with `ORT_DYLIB_PATH` for the fixture only. The default
installed runtime and system CPU runtime 1.23.2 both failed model initialization
with a duplicate-schema error. No installed binary, runtime setting or GPU hold
was changed. The public v0.31.0 archive SHA256 was
`93a18ff467a2f0d8b7cf3acef018ae52f69c69ac6034059978ca525715e8b91c`;
the selected runtime SHA256 was
`e5a7e3646718d8f1f8f52c8fcb770fe229ab44305caf3ea702d558e6e426c9aa`.

Producer source SHA256: `880401b265ea7e820a8130aa97a6de51644d2113dc2a40b7350b4fae7449cbb2`.
Option description source SHA256: `03a676a4b6972ef983dcdc92522baac530ddfc026868634fe558de8ccecce65c`.

[Replayable recording](output.log.gz), [timing](timing.log) and
[readable transcript](transcript.txt) include both probes. Before replay,
run `gzip -dk output.log.gz`. The native portion indexes only the five named
public documents; it proves inclusion and typed-reader survival, not a live
release, outward policy acceptance or whole-repository indexing.
