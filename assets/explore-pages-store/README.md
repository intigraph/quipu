# Pages default-store publication demonstration

Trigger: build Pages while the latest released pack belongs to a legacy engine.
The live base `46a26acd` returns `export default null` and HTTP 404 for the
snapshot. The candidate builds the checkout engine with the existing shared
WASM action, stages it over the released bundle, and requires the producer.
Its isolated published-book output contains a non-null proof and a 39,920,237-byte
snapshot. This recording shows those actual responses, not deployment acceptance.

Demonstrated source: `46a26acd` plus the workflow change in this directory's
introducing commit. The qualified engine/runtime source is byte-identical to
that checkout; local qualification reused those compiled artifacts. Exact-head
CI must build the engine afresh. The same pack is used on both sides.

Download `output.log` and `timing.log`, then replay with util-linux:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

`output.log` is also a readable transcript. Replay was checked; no inline player
is claimed. These small files are retained in the existing assets directory.
To repeat against an isolated book produced by the candidate workflow:

```sh
EXPLORER_DIR=/path/to/docs/book/book/explore bash run.sh
```

The baseline assertion deliberately fails once live Pages changes. The separate
23-control browser qualification measured 2.003 seconds cold and 1.364 seconds
warm including a positive native query and polling. These Linux fixture timings
are not exact DOM-ready measurements or live Mac acceptance.
