# Explicit substring search

An optional SQLite FTS5 trigram index accelerates current, local subject-only
projections of plain-string substring matches. Queries do not create or backfill
the index. Unsupported expressions, historical reads and composed stores retain
the ordinary evaluator.

Prepare an owned, quiescent database copy with the matching binary:

```sh
quipu search-index backfill --kind substring --batch-size 10000 --db COPY.db
quipu search-index status --kind substring --db COPY.db
```

Each invocation processes one bounded batch; an operator decides whether to run
another. The CLI retains its existing protected ingest-minute refusal. `status`
opens the store through the maintenance path; it is not a promise of a file-free
observation. A complete, supported schema is required for `ready: true`.

The derived tables and transaction triggers track current inserts, retractions
and rollbacks. They are omitted from logical exports; receivers prepare their own
index. Exact RDF equality is checked after fingerprint collisions. Language and
typed literals retain ordinary SPARQL semantics.

Clients requiring this path send `X-Quipu-Require-Substring: v6` on `/query`.
The server checks completeness before evaluation, pins readiness and facts in
one SQLite read snapshot, and echoes `X-Quipu-Substring-Index: v6` on success.
Missing or incompatible indexes refuse; they do not silently trigger a scan.
This contract excludes historical, federated and attached-provider reads.
Its connection-local SQLite cache is capped at 8 MiB, preserving smaller caller
budgets; cache and mmap policies are restored after success, error or unwind.
This bounds that cache, not the whole query's memory or result size.

To remove the optional index, first stop dependent full-field consumers and
quiesce the database, then use the matching binary:

```sh
quipu search-index drop --kind substring --db COPY.db
```

Remove the derived triggers before rolling back to a binary that lacks their
codecs. Facts remain intact, and requiring clients then refuse until preparation
is explicitly restored. Production adoption is a separate operator decision.

See the [synthetic before/after demo](../../../probes/pr-demos/substring-search/README.md).
