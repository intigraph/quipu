# Schema migration review repairs

The recording runs the same isolated controls against the retained failing
product and the repaired candidate. No installed service is called.

Before: `78f832815c56c3744efc37832e4e848635013b56`, with only the probe tests
added. Four migration assertions fail and two paired controls pass. A defeated
subject filter also fails with a complexity refusal under a three-row join cap.

After: product commit `04d385907f18d41e236eeb84056efa1cd9dff1be`, which includes
current main `6ba582d19e405a977d7bda5ac1f5c09702993bb9`. All six migration controls
pass, as does the identical filtered scan. The candidate product bytes match the
recorded source witness
`0c709581e040e4678a769b75a7ca9918cba11af2a337a61dd003b48e79f261e1`.
The subsequent recording-packaging commit changes only this directory.

Watch the refused commit return to autocommit with a successful subsequent write,
invalid restricted XSD casts being refused, preview matching acceptance when
write validation is disabled, and an accepted program resisting replacement.
The valid unsigned-byte cast and existing savepoint helper remain passing
controls. Migration scan ceilings and the stricter configured join cap remain
in force; the ordinary read does not acquire a migration-only scan ceiling.

[Readable transcript](transcript.txt), [recording](output.log.gz), and
[timing](timing.log) are retained in this directory with the repository.
This is a terminal recording, not an inline video. Download the files and replay:

```sh
gzip -dc output.log.gz > output.log
scriptreplay --log-out output.log --log-timing timing.log
```

The raw recording and accelerated playback were inspected. It contains synthetic
`example.org` fixtures and expected failing controls; its final exit zero means
the demonstration completed, not that the baseline tests passed.

Run the candidate controls from its source checkout:

```sh
cargo test --locked --features shacl --lib lex_review -- --test-threads=1 --nocapture
cargo test --locked --features shacl --lib ordinary_filtered_scan_keeps_existing_join_budget_semantics -- --nocapture
```

For the baseline, use a separate checkout at the before revision and apply only
[the probe patch](baseline-probes.patch.gz), then run those same commands.
The patch was checked against those exact baseline test files. It changes tests
only. Binaries are not distributed in this directory.

These are focused source controls, not a full integration run or evidence of
production migration activation. The full release performance gate, hosted CI,
and independent review are separate validation steps.
