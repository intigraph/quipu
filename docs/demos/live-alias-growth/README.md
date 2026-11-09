# Post-open dictionary aliases

This disposable SQLite library probe opens a composition, then interns one IRI
independently in both files. Watch `late-shared-canonical` change from `false`
to `true`. The second case first observes a remote-only spelling, then interns
the local spelling: `later-local-spelling-canonical` also changes to `true`.
The different-ID, unchanged-local-ID and remote-only controls pass on both sides.
No service, credential, routed graph or production database participates.

The recording compares library source `6411b3ed322edc9022791bdef2cee3ed1851a8dd`
with candidate `0eea52a7b7e4bb9ab1d33fc7957a4f7a70d31d0f`, using the same
[`alias_growth`](../../../examples/alias_growth.rs) executable source.
Both builds use `--no-default-features --features shacl`, dev/unoptimized.
The new example is applied to the baseline library source for that build; these
are instrumented source builds, not clean deployment binaries. The candidate's
library implementation is unchanged by the later recording/documentation commit.

Reproduce the candidate:

```sh
cargo run --no-default-features --features shacl --example alias_growth
```

To compare, build the same example against the baseline library source and the
candidate in separate checkouts, retain both binaries, then run:

```sh
BASE_PROBE=/path/to/base HEAD_PROBE=/path/to/head bash run.sh
```

Download [the plaintext output recording](output.log) and [its actual timing](timing.log),
then replay with `scriptreplay --log-out output.log --log-timing timing.log`.
The recording is 658 bytes; SHA-256
`1df97da5fa64eb9aaa10656ca858f2a7126b9418aa2f6a2a8280dc74baf82e8f`.
It is the exact decompressed recording, not a new execution; its normalized
bytes match the existing transcript. The compressed source member was removed.
Use `--divisor 0.1` to slow the short fixture. The [transcript](transcript.txt)
normalizes CRLF for reading. No inline player is claimed. These small sanitized
artifacts follow this repository's existing `docs/demos` retention.

This demonstrates the canonical-ID API fix. It does not establish every composed
query's parity, a common cross-file cut, durable writer fencing or pilot activation.
