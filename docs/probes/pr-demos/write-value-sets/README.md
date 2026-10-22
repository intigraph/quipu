# Cross-write language-tag uniqueness

The input writes two different English literals to one focus governed by
`sh:uniqueLang true`. Whole-data validation rejects the pair. Before this fix,
splitting it into two writes incorrectly accepted both; after it, the second
write is rejected. The reversed split is checked too. The deliberately weak
payload-only control continues to accept, proving that the input distinguishes
the contextual check from validation of each payload alone.

This is an isolated native source fixture, not an installed-service test. The
base is `0e89d729e254430806853ade78f9a7287de9e921`. Both adapters use the identical
`--write-path` instrument from PR #707 at
`7bb51180320dc8cebacc2d93ae19197889d10d52`; only the candidate's two post-state
library files differ. Build each adapter with
`cargo build --no-default-features --features shacl,dev-tools --bin quipu-shacl-conformance`.
Then run:

```sh
python3 run.py --before /path/to/base-adapter --after /path/to/candidate-adapter
```

The candidate also covers disjointness and strict/non-strict property comparisons;
`tests/write_value_sets.rs` contains their refusal and valid-input controls,
rollback checks, graph isolation and unsupported-path refusal cases.

A small output-only terminal recording is retained here as `output.log` plus
`timing.log`; it is not a video or an inline player. Download both and replay:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The readable transcript is the fallback. The PR identifies the exact candidate
head. These small source-fixture artifacts remain with the source; they contain
no credentials or private infrastructure identifiers. They do not certify
production validation configuration or whole-store conformance.
