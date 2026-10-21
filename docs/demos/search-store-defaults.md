# Stored search defaults demo

Implementation revision: `65ae5a9e80e9116636ae83254cd84fbca00eefda`, based on
`afda438b89648d9622c8724f35806cc92087b854` (proposal plus main
`0a20f0a1363e2fa628b19e6c256c6c4060067566`). Target: isolated SQLite fixtures.

A store created from the frozen pre-policy schema keeps ROOT when configuration
omits `default_graph_scope`. A new store uses ALL. Watch `search_scope.default`
and `default_source`: the same keyword request returns one versus two records.
Once fixture vectors are prepared, the default semantic top hit stays ROOT for
the existing store and becomes the named record for the new store. Explicit ALL
returns the named record in both. An unprepared new-store semantic request
refuses; it does not silently report ROOT as complete coverage.

Before the migration, the unchanged legacy semantic regression failed with
named-graph search disabled. With this revision it passes. The recording runs
the candidate's two fixture cases; it does not replay the earlier failing build.
It additionally queries both candidate databases using a copied clean
`0a20f0a1363e2fa628b19e6c256c6c4060067566` CLI. Both positive ROOT queries pass,
and facts, store identity and stored policy remain unchanged.

[Actual recording](search-store-defaults.txt.gz) and
[timing](search-store-defaults.time) are small retained repository artifacts.
Playback requires download, decompression and a terminal replay tool:

```sh
gzip -dc docs/demos/search-store-defaults.txt.gz > /tmp/search-store-defaults.txt
scriptreplay --log-out /tmp/search-store-defaults.txt --log-timing docs/demos/search-store-defaults.time
```

Reproduce with a separately retained pre-migration CLI; the driver refuses
fixture file overwrite. Run from a fresh disposable directory:

```sh
cargo build --features remote --bin quipu --example search_graph_defaults_fixture
repo_root="$PWD"
# Set prior_cli to the retained pre-migration executable, outside this directory.
mkdir search-default-demo
cd search-default-demo
QUIPU_FIXTURE="$repo_root/target/debug/examples/search_graph_defaults_fixture" \
QUIPU_PRIOR="$prior_cli" python3 "$repo_root/examples/search_graph_defaults_demo.py"
```

The recording and driver use synthetic data and no network. They prove source
behavior and older-reader compatibility for the fixture. They do not prove
production vector readiness, installed service behavior, or authorization.
