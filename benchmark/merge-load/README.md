# Shipped merge-path load test

This complements [mergebench](../mergebench/README.md). That paper arm compares
merge strategies on generated graphs; this arm drives the shipped native
reconnect, delta, Explorer WASM and Git transports. It makes no LLM calls,
network graph requests, model loads or schedule changes.

## Repeat

```sh
just bench merge-load-build
just wasm explorer-node
target=$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
uv run --with rdflib python benchmark/merge-load/run.py \
  --driver "$target/debug/examples/merge_load" --quipu "$target/debug/quipu" \
  --wasm-module "$PWD/wasm/explorer/pkg-node/quipu_wasm_explorer.js" \
  --out benchmark/merge-load/out/run42
```

The output directory must not exist. Retain failed runs; use another name for
a corrected run. Default sizes are **100, 1000 and 5000 edits per replica**,
seed 42, with 100-edit delta chunks. `--sizes`, `--seed`, `--chunk`, and
`--paths native,delta,wasm,git,replicas` make the scope explicit. CI runs all
five paths at 1000 edits. `--timeout` bounds each child operation; a timeout
or unsupported path is not a successful merge. `/usr/bin/time`, Git, Node,
Python with RDFLib, and the actual built binaries are prerequisites.

## Recorded demonstration

The [22.7-second terminal recording](demo.cast) runs the actual native CLI and
Explorer WASM at 100 edits per side, seed 42. Watch both full-fact oracles pass,
with ten conflicts, eleven labels and fact-idempotent replay. This is a new
coverage harness; there was no corresponding five-path load matrix on the base.

The product source is `ccbf48b4ef4ec8558305fdee4932d63c14258b8e`; the executed
runner SHA256 is `8fe43bb1e5c9f629463f1b39eb498da70cb6095b9b475ae3b66d029f766b2d60`.
The actual WASM SHA256 is `d4e5db85cb8a863c57af6b2843f4f249d4afa6e1946dadc3741a39a7ec54cba3`.
Run `demo.sh` with `MERGE_LOAD_DRIVER`, `MERGE_LOAD_QUIPU` and `MERGE_LOAD_WASM`
pointing to those built artifacts, or use the repeat command above.
The v2 cast was converted losslessly from real GNU script output and timing;
reconstruction against the captured bytes was verified. Replay with an asciicast
viewer (for example `asciinema play demo.cast`); no inline forge player is assumed.
Fixtures are private. This proves source paths, not installed runtime recovery.

## Independent oracle

`model.py` generates an operation ledger, not expected merger responses. Each
replica has exactly N logical edits: additions, pure retractions, replacements
and 10% deliberately shared maxCount-one replacements. Other edited slots are
disjoint. The reference policy applies those declared operations, retains all
unrelated base facts, and selects replica zero at the seeded conflicts. The
Git driver's unresolved artifact instead holds those slots at base.
Fixtures seed independent physical stores with identical base RDF and distinct
store identities for native reconnect. Git branches deliberately retain their
pendant producer identity: a separate negative control must refuse a foreign
producer, without changing the worktree or index. This is a real transport
boundary, not permission to rewrite another producer's manifest identity.

Full RDF facts are compared through RDFLib, independently of Quipu's parser,
merge algebra and cardinality implementation. Counts alone never pass. The
oracle controls inject missing, extra and resurrected facts and prove they
fail; they also verify exact overlapping slots, workload counts and seed
reproducibility. Actual `--emit-decisions` files must contain exactly the
seeded conflict rows and readable subject/predicate labels. Native decisions
are applied through the real decision API, then the input is replayed and
the entire fact set must remain unchanged. A replay may still return a
conflict requiring review; that is not called an automatic resolution.

Synthetic cross-repository mapping records use the existing SSSOM predicates
`subject_id`, `object_id`, `predicate_id`, `mapping_justification`, with
`skos:closeMatch` and manual-curation justification. They test lossless fact
transport, **not** an approved cross-stack mapping dataset, automatic identity
inference, or real repository namespace authority. The existing synthetic
scrub catalogue is included so outward producers execute their normal checks.

## What each path proves

- **native:** actual Store/share with `parent_share`, reconnect, emitted
  decisions, apply and fact-idempotent replay.
- **delta:** real small delta artifacts are materialized and reconnected at
  every step; every prefix matches its ledger. One large delta from the same
  base must match the final chain and resolved merge exactly.
- **wasm:** the actual built `Explorer` runs in Node's WebAssembly VM, loads
  the archive, performs offline retract/knot edits, calls `ex.delta()`, then
  materializes and merges the emitted files natively. This measures the
  shipped WASM engine and interchange, **not** browser DOM, OPFS, HTTP or UI.
- **git:** real branches and `merge=quipu` run through `quipu git-merge` and
  its installed-per-command driver. The entire held-conflict graph is checked;
  `pendant-check` must refuse that unresolved commit. Replay must preserve it.
  This does not claim that a human resolved hundreds of decisions in the UI.
- **replicas:** five offline stores are successively merged, checking the
  complete oracle after each replica, including every retraction and mapping.

## Measurements and evidence

`results.json` records source revision, artifact hashes, scope, per-operation
wall time and child peak RSS, store growth, decision row/byte/label counts,
replay outcomes, failures and retained raw request/output files. Row wall time
includes setup and oracle work; merge samples distinguish the actual path.
`max_child_peak_rss_kib` is the largest individual child peak, not an aggregate
of concurrent processes. `oracle_process_peak_rss_kib_cumulative` is Python's
process high-water mark across rows, not an isolated row measurement. Scratch
bytes include logs/artifacts; store growth separately measures the database.
Render a table from those exact results with
`python3 benchmark/merge-load/report.py <run-directory>/results.json`.

Facts and edit schedules reproduce by seed. Store identities, transaction
provenance, timings and platform memory measurements need not be byte-identical.
Record compiler/profile and hardware with any published timings; debug/native
and size-optimized WASM figures are not a runtime-speed comparison.

The harness never publishes findings itself. Capture each completed run's
scope, artifact identity, results and limitations through the normal governed
Quipu episode path. Do not turn source fixtures into production acceptance.

The explorer lock can be refreshed deliberately with `just wasm explorer-lock`;
the actual build always uses `--locked`. A stale lock is a source setup failure,
not permission to silently resolve dependencies during the measured build.
