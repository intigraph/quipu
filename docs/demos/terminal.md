# Insert a fact, watch the graph change

The 24-second `terminal/demo.svg` records real local commands: it shows two
plain Turtle facts, knots each into a fresh store, queries the result, then
reads the store back at transaction 1. `terminal/graph-before.svg` and
`terminal/graph-after.svg` draw the rows those native reads returned, in a
strand-and-knot motif; they are not explorer screenshots. The plain transcript
is `terminal/transcript.txt`. The fixture is public and uses only `urn:demo:`
identifiers. No server, production store, token or embedding model is used, and
no command writes to stderr: the recorder refuses any step that does.

From the repository root, use a new private workspace and a native Quipu CLI:

```sh
python3 docs/demos/terminal/prepare.py /tmp/quipu-demo
python3 docs/demos/terminal/render.py docs/demos/terminal/demo.tape.json \
  --workspace /tmp/quipu-demo --tool quipu="$(command -v quipu)" --tool cat=/bin/cat \
  --output docs/demos/terminal
python3 docs/demos/terminal/graph.py
python3 docs/demos/terminal/test_harness_origin.py
```

`prepare.py` refuses an existing workspace. Do not reuse a prior database:
the transaction numbers in the tape assume a fresh store. The recorder refuses
unexpected exits, missing expected output, any stderr or possible private data
before replacing artifacts. Execution times are in the
manifest; the 24-second timeline is deliberate reading time. The native binary
version and SHA, tape, renderer, fixture and generated-asset hashes are bound
there. `graph.provenance.json` binds both graphs to that manifest and its exact rows.
`cat` is recorded with `version_args` of `/dev/null`, because BSD `cat` has no version flag.

This vendors the byte-identical shared Tambo terminal recorder at
`docs/demos/terminal-harness/render.py`; its origin commit and SHA are recorded
in `terminal/harness-origin.json`. Future changes should come from that shared
source. It uses Python's standard library, an isolated HOME and an explicit
`.terminal-demo-sandbox` marker. It is a recorder, not an OS security sandbox;
review tapes before executing them. Every visible frame is leak checked, wraps
at 65 columns and uses the same opaque 24px theme. The last frame is the
non-animated SVG fallback. No remote assets, README edits or new CI workflow
are introduced. README placement is a separate step.
