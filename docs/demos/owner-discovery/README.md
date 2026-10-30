# Owner discovery regression demo

Three synthetic fixtures expose unnecessary work in current named-graph reads:
dense literal history, predicate-presence payload reads, and status decoding for
unrelated owners. The same tests fail on the base and pass on the candidate.

- Before: `6ba582d19e405a977d7bda5ac1f5c09702993bb9`, with only the two
  regression-test files from the candidate applied.
- After: `76a408af0311a2beef2f8d4259ed8dcde4f22716`.
- Target: default-feature Rust library tests against isolated synthetic fixtures.
- The later recording/caption commit changes no runtime code.

The [terminal recording](recording.cast) preserves the output and timing of a
util-linux `script` capture, converted to asciicast v2. The [transcript](transcript.txt)
normalizes CRLF for reading. The original capture replayed successfully with
`scriptreplay`; conversion was checked against every captured output byte.
Download the cast and use `asciinema play recording.cast` with a compatible
player. Inline playback is not claimed.

The failures to watch are 42,648 VM instructions for the dense-history lookup,
a fact-table column read for predicate presence, and 12,014 unrelated status
decodes. Each becomes a passing result. The tests also check the expected row,
empty-owner or missing-predicate controls, and fallback behavior.

To repeat, apply the test-only diff to a separate checkout of the before revision:

```sh
git diff 6ba582d19e405a977d7bda5ac1f5c09702993bb9 \
  76a408af0311a2beef2f8d4259ed8dcde4f22716 -- \
  src/sparql/graph_membership/tests.rs src/sparql/status_pushdown_tests.rs
```

Run these filters on each checkout under the repository's test isolation guard:

```sh
cargo test --locked --lib dense_historical_object_lookup_does_not_walk_payload_history
cargo test --locked --lib unbound_predicate_presence_keeps_reads_on_covering_indexes
cargo test --locked --lib absence_with_selective_owner_does_not_decode_unrelated_statuses
```

The recording runs the compiled test executables directly to omit compilation
noise and checks each expected exit status. It contains synthetic data only.
This demonstrates source behavior, not deployment or live workload acceptance.
The small recording and caption are retained in this directory with the PR.
