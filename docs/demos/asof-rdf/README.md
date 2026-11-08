# Retained-cut RDF conversion

This candidate-only recording runs the actual `asof-rdf` command on a private
one-record fixture. A corrupt graph digest is refused without `complete.json`;
the corresponding complete cut writes one Turtle chunk and a transaction-bound
completion receipt. No ambient database is created.

The binary was retained from the guarded feature qualification. Its exact SHA256
is printed in the [output](output.log). The candidate Rust files are unchanged
from the qualified source tree `9b3ebe63a2923deafb69424e886bd51e78ff3912`.
The base has no `asof-rdf` command, so this compares valid and corrupt input on
the candidate rather than claiming an equivalent base-binary run.

Download [output](output.log) and [timing](timing.log), then replay:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Repeat with an already qualified binary; this script does not build one:

```sh
python3 docs/demos/asof-rdf/demo.py --binary /path/to/quipu
```

This is a replayable terminal recording, not an inline video. It proves tiny
fixture conversion and refusal, not source authentication, full-size resource
limits, production acquisition, installed capability, or whole-graph SHACL.
