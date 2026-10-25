# A note becomes a queryable graph

The 20-second `terminal/demo.svg` captures a real local episode and query.
`terminal/graph.svg` draws the row actually returned by that query in a
strand-and-knot motif; it is not an explorer screenshot. The plain transcript
is `terminal/transcript.txt`. The fixture is public and uses only `urn:demo:`
identifiers. No server, production store, token or embedding model is used.
The native missing-model warning is included: loading a graph is not proof of
semantic-index readiness.

From the repository root, use a new private workspace and a native Quipu CLI:

```sh
python3 docs/demos/terminal/prepare.py /tmp/quipu-demo
python3 docs/demos/terminal/render.py docs/demos/terminal/demo.tape.json \
  --workspace /tmp/quipu-demo --tool quipu="$(command -v quipu)" \
  --output docs/demos/terminal
python3 docs/demos/terminal/graph.py
python3 docs/demos/terminal/test_harness_origin.py
```

`prepare.py` refuses an existing workspace. Do not reuse a prior database:
episode upsert is intentionally idempotent and would not reproduce the fresh
14-fact ingest. The recorder refuses unexpected exits, missing expected output
or possible private data before replacing artifacts. Execution times are in the
manifest; the 20-second timeline is deliberate reading time. The native binary
version and SHA, tape, renderer, fixture and generated-asset hashes are bound
there. `graph.provenance.json` binds the graph to that manifest and exact row.

This vendors the byte-identical shared Tambo terminal recorder at
`docs/demos/terminal-harness/render.py`; its origin commit and SHA are recorded
in `terminal/harness-origin.json`. Future changes should come from that shared
source. It uses Python's standard library, an isolated HOME and an explicit
`.terminal-demo-sandbox` marker. It is a recorder, not an OS security sandbox;
review tapes before executing them. Every visible frame is leak checked, wraps
at 65 columns and uses the same opaque 24px theme. The last frame is the
non-animated SVG fallback. No remote assets, README edits or new CI workflow
are introduced. README placement is a separate step.
