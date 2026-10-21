# A shared governed memory engine

The same constructor probe shows the change: the before bundle exports the
existing `Explorer`, while a general `Database` is absent. The after bundle
creates an empty memory store, refuses a data write before explicit shape
adoption, admits a valid record afterward, and rejects a missing title without
leaking a second record. History retains the exact integer token
`9007199254740993`.

This is an actual Node/WASM run over isolated memory stores. The recording's
before bundle was built at `5e760ec468a220a4145572faae418774805aed69`; its
`src` and `wasm/explorer` source trees are identical to the PR base
`533c96938232987f5706cc54af99a8ae0c821f29`. The after bundle was built from clean
engine commit `44f58275671e4909012fb0f03434fb78e6781636`, reporting that revision
and `git_dirty=false`. Later recording/caption additions do not change the
engine source.

## Watch or reproduce

Download the [compressed output](output.log.gz) and [timing](timing.log)
together. This is a replayable terminal recording, not an inline video:

```sh
gzip -dc output.log.gz > output.log
scriptreplay --log-out output.log --log-timing timing.log
```

Playback was verified with the installed util-linux player. A
[readable transcript](transcript.txt) is derived from the same recording.
Compression preserves its original mixed line endings through text hooks.
The small recording and caption are retained in Git.

To rerun [the actual reproducer](demo.mjs), compile the two pinned revisions'
`wasm/explorer` crates and generate Node bundles with the matching
`wasm-bindgen --target nodejs` CLI. The new revision provides the focused build
command `just wasm explorer-build`; the baseline uses its existing Explorer
build workflow. Then pass the two generated module entrypoints:

```sh
node demo.mjs before/quipu_wasm_explorer.js after/quipu_wasm_explorer.js
```

The guarded run left all 158 watched HOME paths and five resources untouched.
It proves the memory-store behavior shown here. OPFS persistence, actual Creel
migration, the remaining native operation surface, and Python remain open.
