# Search graph coverage demo

Source revision: `6fa587aa6e56a24097fbfd550d1baa674693f7d0`. Base: `0eaf25f8e0fe6fe8b0164cf76e3a90dba8000e70`.
Target: an in-memory source fixture, not an installed service.

The same `{"mode":"keyword","query":"shared"}` request returns one record
with the ROOT compatibility configuration, then three with the all-graph default.
Watch `search_scope.graphs`: ROOT becomes ROOT plus the two fixture graphs.
This recording compares both configurations in the candidate; it does not run
the base executable. The pre-change graph-scope tests separately verified the
base's one-record default. Index completeness remains explicitly uncertified.

[Recording transcript](search-coverage.txt.gz) and [timing](search-coverage.time)
are small retained repository artifacts, inspected for private data. Playback
requires downloading, decompressing, and using a terminal replay tool:

```sh
gzip -dc docs/demos/search-coverage.txt.gz > /tmp/search-coverage.txt
scriptreplay --log-timing docs/demos/search-coverage.time --log-out /tmp/search-coverage.txt
```

Repeat the fixture through the normal build admission path:

```sh
just test --no-default-features --lib search_graph_scope::default_tests::legacy_root_default_is_configurable_and_bad_configuration_refuses -- --exact --nocapture
```

This supports source review. It does not prove production backfill, deployment,
network recovery, or live named-graph retrieval.
