# CI admission comparison

For an ordinary non-draft core-only PR, the same workflow-expression probe on
base `d39f6312294db3705c41b4357198bd51c14943ca` and the candidate admits
25 → 16 CI job slots. Generated-doc drift remains admitted; seven heavy surfaces
leave that PR. The Python, docs and merge-load workflow changes are additional
reductions outside this CI-only comparison.

Reproduce from the candidate checkout with Python/PyYAML and Node installed:

```sh
just ci demo
just ci test
```

[Readable transcript](transcript.txt). This is a configuration simulation using
trusted workflow expressions, not a hosted run or runner-time measurement.
Matrices are expanded where static; reusable workflow calls count once, and
cross-workflow dispatch, dependency outcomes and scheduler overhead are excluded.
The Wasm selector is fixed to `false` for this core-only input. Python, Wasm and
docs PR path controls and the full nightly negative controls run in `just ci test`.

No replayable terminal recording: asciinema is unavailable in the authoring
environment. The transcript preserves real probe output; it is not a video.
The PR description identifies the exact candidate commit demonstrated.
