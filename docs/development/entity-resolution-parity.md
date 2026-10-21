# Exact entity-resolution parity controls

The label matcher uses conservative upper bounds before and during Jaro-Winkler
matching. A retained match must have exactly the score bits returned by the
locked `strsim` implementation. Token or trigram overlap is not a safe filter:
transpositions can produce qualifying matches without a shared trigram.

Name candidates retain their term IDs until selection. Selection resolves whole
score tiers, including the boundary tier, then applies exclusions and deduplicates
resolved IRIs. It never assumes different term IDs imply different IRIs. The
ordinary resolver preserves its IRI tie order; the in-lock delta check preserves
its stable input tie order. Zero-output and unordered-score cases retain the
legacy full-resolution path.

The normal resolution tests cover score boundaries, Unicode lowercasing,
transpositions, buffer reuse, alias IDs, exclusions, duplicate candidates and both
tie policies against an eager reference selector.

```sh
just test --locked --no-default-features --lib resolution::
```

## Optional large offline label fixture

An ignored test compares the complete old and new label-candidate lists for the
supplied episode names plus 1,000 seeded existing-label samples. It checks IDs,
score bits, exact flags and input order. It requires at least 600,000 label rows
and never opens a graph, model or endpoint.

The label file contains one `<integer-term-id>|<hex-encoded-Value>` per line;
string values have tag byte `01`. Preserve all current label rows and their
order, including duplicates. The JSON episode fixture supplies `nodes[].name`.
These inputs may be private: do not commit an operational label dump or episode.
Use an existing executable and your normal test filesystem/resource guard when
available; the following command compiles the test through the usual build path.

```sh
QUIPU_RESOLUTION_LAB_LABELS=/path/to/offline-labels.hex \
QUIPU_RESOLUTION_LAB_PAYLOAD=/path/to/offline-episode.json \
QUIPU_RESOLUTION_LAB_SAMPLE=1000 \
just --command cargo test --locked --release --features full --lib \
  resolution::index::lab_tests::offline_existing_labels_score_bits_and_order \
  -- --ignored --exact --test-threads 1 --nocapture
```

`LAB_PARITY` reports component scan time only. It excludes store startup, label
index construction, SQL ID-to-IRI resolution, embeddings and both complete caller
pipelines. It is not end-to-end hint parity or a production performance result.
Those require a separately admitted, representative offline store comparison.

## Exact vector batch access

The SQLite current-vector batch scan uses a partial covering index on entity ID
and embedding, avoiding each vector row's potentially large text payload. Normal
writable store initialization builds the index; this can add startup time and
substantial derived storage, which must be measured separately from request time.
SQLite maintains it on insertion, closure, replacement and rollback. It is not
an approximate index or a process cache. Queries never build it.

The indexed scan feeds SQLite rowid into the unchanged top-k comparator as the
legacy tie key, avoiding a SQL sort of the large payloads. Finite scores therefore
retain exactly the original ordering. Non-finite inputs, unrepresentable rowids
and dimension errors use the original row traversal so unordered comparisons and
first-error identity remain unchanged. Historical scans retain their original path. Read-only stores without the new
index use the previous scan. Tests compare complete results, score bits and
order across both paths, including validity changes, rollback and errors.

Removing the index through a separately approved maintenance path returns
queries to the legacy scan; an older binary can also ignore the derived index.
Source rollback does not automatically reclaim its storage. A faster vector
scan alone does not establish the full episode performance target.
