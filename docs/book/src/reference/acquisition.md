# Atomic source acquisition

`POST /acquire` returns complete canonical N-Triples for explicitly selected
named graphs, plus a separate `quipu-acquisition/v1` source receipt. A consumer
must recognize that contract in `/version.read_contracts` before using it.
Historical acquisition through `/share` is unsupported.

```json
{
  "source_store_id": "the-known-source-lineage-id",
  "graphs": [
    "https://example.org/project/board",
    "https://example.org/project/comments"
  ],
  "tx": 42,
  "max_bytes": 262144
}
```

The expected store identity and exact project/comment graph IRIs come from
trusted consumer configuration. The endpoint requires 1–16 distinct registered
committed graphs. It never expands a dataset, includes ROOT implicitly, follows
an overlay, accepts arbitrary queries, or pages its response. An unknown graph
is refused rather than represented as an empty successful acquisition. A graph
IRI that also names a dataset alias is refused: `/query` would expand that alias,
which could mislabel another graph's data as the requested source graph.

`tx` must be a positive signed 64-bit JSON integer naming an existing committed
transaction. Null, strings, fractions, negative values, unknown transactions,
and future transactions are refused. Omit `tx` to capture present state.
Unknown fields, including `valid_at`, `fork`, and per-graph clocks, are refused.
Attached stores are unsupported by this contract, including for present
acquisition: a main-file transaction cannot establish another file's generation.
Ordinary unpinned `/query` reads remain available on attached stores.

The producer opens one SQLite read transaction before reading its identity and
head. Every graph is read at `applied_tx`, and the transaction remains open
through canonicalization and receipt construction. A concurrent writer can
commit, but that commit cannot alter part of the acquisition. The complete
response is capped at 8 MiB; `max_bytes` can only lower that ceiling. Query row
ceilings and deadlines also apply. An oversized, truncated, timed-out, or refused
read produces an error and no complete receipt. Consumers must treat errors,
missing fields, and incomplete transport bodies as UNKNOWN, never empty success.

The response has three members:

- `graphs`: a map from each exact graph IRI to its canonical N-Triples string;
- `source_receipt`: the source identity, transaction selection, build, and scope;
- `source_receipt_sha256`: SHA-256 of compact UTF-8 JSON of the receipt, with
  recursively sorted object keys and no trailing newline. Strings retain UTF-8
  characters rather than ASCII escapes; quotes and control characters use JSON
  escaping. This matches the producer's deterministic share-manifest encoding.

The receipt contains `schema`, `source_store_id`, `source_head_tx`,
`requested_tx`, `applied_tx`, `history`, `history_min_tx`, `canonicalization`, `producer_version`,
`producer_git_sha`, `producer_git_dirty`, `graphs`, and `complete`. Its graph map
records each graph's triple count and SHA-256 of the exact N-Triples bytes.
`canonicalization` is `source-labels-sorted-ntriples/v1`: triples are deduplicated
and sorted by their serialized bytes, preserving the source's blank-node labels
across graphs. Independently renaming blank nodes within each graph could break
cross-graph identity. Consumers must import these labels as one acquisition
document scope, preserving identities across its graph members. This is not an
RDFC-1.0 graph-isomorphism digest.
`requested_tx` is null for a present-state acquisition; `applied_tx` then equals
the captured `source_head_tx`. For a historical request, `applied_tx` equals the
validated requested transaction. Both modes use the same pin for every graph.

`history` is `file-local-bitemporal-eavt`; the retained transaction-log range is
`history_min_tx` through `source_head_tx`. Existence is checked separately, since
an integer inside that range need not identify a committed transaction. The
source head is a captured transaction generation, not a substitute for a
requested pin or a content hash. The receipt hash binds the build, source,
generation, exact graph scope, and graph content identities together.

Store identity records lineage: a file copy shares its source store ID. The
receipt does not certify a unique physical host, an operator's authority over an
endpoint, or a trusted producer signature. Hashes detect altered graph bytes or
an altered receipt against its expected digest; they cannot authenticate a
hostile producer that replaces both. Consumers must retain their endpoint trust
and expected source identity and inspect the producer build separately.

Native consumers can use `quipu::acquisition::Payload::verify` with their original
request before importing any bytes. Verification checks the recognized contract,
completeness, expected source/pin/scope, receipt digest, canonical graph bytes,
content digests, and triple counts. Retain the source receipt and its digest
unchanged beside any generated local artifact. That artifact's local store ID,
scope, and transaction anchor describe the artifact; rewriting them to resemble
the source would discard the distinction the receipt preserves.

`/query` also validates its `tx` field without coercion. A transaction pin cannot
be combined with `valid_at` or `fork` at this remote boundary. This restriction
does not change the internal bitemporal evaluator's independent time axes.
