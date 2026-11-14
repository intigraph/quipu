# Creator retention demonstration (transcript)

## Trigger and target

Prune a registered graph's creation event, restart, then repeat create with a
different caller. The original actor should survive; the later caller must not
replace it.

Before: a private scratch store using the clean `99c777e5` installed CLI/server
pair, whose Rust/Cargo/build inputs match base `28ad788f`. The unchanged book
recipe used its October 14 example timestamp, now beyond default retention.
This is the actual diagnostic output:

```text
RESTART_ACTUAL {"created": false, "creation": {"actor": null, "timestamp": "2026-10-14T12:00:00Z"}, "g": 2, "graph": "urn:example:graph:job-search"}
AssertionError
```

After: source `635fdc2fe1626a8782fedadf101d956dd4d53448`, isolated native
store controls. The focused test creates actor `original` at
`2000-01-01T00:00:00Z`, prunes through `2001-01-01T00:00:00Z`, verifies the event
log is empty, reopens the store, and repeats create as `later-caller`. It checks
that the original actor and timestamp remain. Actual qualification excerpt:

```text
test store::graph_creators::tests::creator_survives_real_pruning_restart_and_repeat_without_retaining_events ... ok
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 4227 filtered out; finished in 1.23s
```

Repeat the focused after-control from the candidate source:

```sh
cargo test --locked --lib store::graph_creators::tests::creator_survives_real_pruning_restart_and_repeat_without_retaining_events -- --exact
```

## Limits and retention

This is a real transcript, not a terminal recording: `asciinema` was unavailable.
The before probe uses MCP; the after control exercises the underlying store API
and assertions, rather than printing an equivalent MCP response. It proves
candidate behavior, not a deployed migration or restoration of previously lost
actors. Both fixtures were private, and the HOME/resource guard was clean.

Retain this small sanitized transcript with the repository's design documents.
Later transcript-only commits do not change the demonstrated Rust source.
