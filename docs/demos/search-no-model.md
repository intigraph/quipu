# Search without a configured model

The same fresh ingest and `quipu search Clockwork --db fresh.db` changes from
`keyword search is disabled` to one keyword hit. `doctor search` reports keyword
readiness and `semantic_ready: false`; no model is downloaded by either command.

Base: `277918020f4734c29582fb8e34722c59f510c8dc`.
Implementation: `1ef3a0556f004fd7a50b4a0c598c17b1131d4516`.
Both executables report clean source revisions. Targets are separate disposable
SQLite stores with synthetic RDF, isolated HOME and XDG directories, and no
configuration or model. This demonstrates source behavior, not deployment.

[Recording](search-no-model.txt.gz), [timing](search-no-model.time), and
[repeatable driver](search-no-model.py) are retained small repository artifacts.
The recording was inspected and replayed locally; playback requires downloading,
decompression, and a terminal replay tool. No inline player is claimed.

```sh
gzip -dc docs/demos/search-no-model.txt.gz > /tmp/search-no-model.txt
scriptreplay --log-out /tmp/search-no-model.txt --log-timing docs/demos/search-no-model.time
```

To repeat, point `QUIPU_DEMO_BASE` and `QUIPU_DEMO_HEAD` at the corresponding
executables, then run `python3 docs/demos/search-no-model.py`.

Keyword hits do not manufacture cosine similarity. The recording deliberately
shows `context_results: []` and `similarity_unavailable`: MCP context admission
continues to require its existing evidence. Explicit semantic mode still needs
a working embedding provider. This demo does not qualify runtime packaging,
semantic backfill, GPU performance, or production search readiness.
