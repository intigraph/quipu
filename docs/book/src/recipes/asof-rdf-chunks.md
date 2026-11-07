# Coherent RDF Chunks

`qp asof-rdf` converts one **retained** `/share/asof` acquisition into bounded
compact-v1 Turtle chunks. It is offline: it does not request a snapshot, follow
a stream, open the configured database, run SHACL, or activate a schedule.
Acquisition authentication, source admission, server spool/WAL sizing and the
validator's resource admission remain separate responsibilities.

The input is JSONL, one object per transport frame:

```json
{"event":"quipu.asof.part","data":"EXACT original JSON data string"}
```

Preserve `data` byte for byte. Parsing and re-serializing it before capture can
invalidate the shared applier's digest chain. A single-frame `quipu.asof` or
one uninterrupted `begin`/numbered `part`/`commit` sequence is accepted. Missing,
reordered, mixed-transaction, additional or malformed frames refuse the cut.

The private binding file supplies the exact source, epoch, transaction, graph
and profile hash, plus aggregate resource limits. For example:

```json
{
  "store_id": "SOURCE_STORE_ID",
  "log_epoch": "SOURCE_LOG_EPOCH",
  "watermark_tx": 123,
  "graph": "https://example.org/graphs/board",
  "shapes_hash": "sha256:PROFILE_HASH_AT_TRANSACTION_123",
  "max_input_bytes": 1073741824,
  "max_frames": 10000,
  "max_disk_bytes": 4294967296,
  "wall_seconds": 1200
}
```

These are example budgets, not measured sizing or an authorization to run.
Get the complete profile **as of the same transaction**, using the source's
`shapes_hash` concatenation convention. A current profile read before and after
acquisition does not establish the historical profile. The binding is checked
against the manifest; it does not authenticate itself or verify a signing key.
Keep the authenticated acquisition receipt alongside it. This command does not
claim cryptographic source authentication from a locally supplied JSON file.

Use private owner-only regular input files and an empty owner-only canonical
scratch directory on the admitted filesystem. Set `SQLITE_TMPDIR` to that same
directory **before** starting the command:

```bash
SQLITE_TMPDIR="$SCRATCH" qp asof-rdf \
  --binding "$BINDING" --frames "$FRAMES" --scratch "$SCRATCH"
```

No existing output is overwritten. SQLite main and temporary databases have
page ceilings; the temporary reserve, retained input, output and database sidecars
are charged against the disk budget. Each input frame is at most 1 MiB. Each
output chunk still uses the existing **256 MiB** compact RDF ceiling. The whole
cut may contain many chunks; it is never assembled into one in-memory graph.

`chunk-00000000.ttl`, subsequent numbered chunks, and a private scratch replica
are staged as processing proceeds. **Only successful completion plus a valid
`complete.json` admits their union.** A partial directory is UNKNOWN, even if
some Turtle files parse or the private replica already committed. Keep partial
artifacts for diagnosis; this command performs no cleanup or automatic retry.

Dictionary prefix IDs are **local to each chunk**. A consumer must parse each
chunk independently or remap its dictionary into a global disk index. Concatenating
the Turtle files would incorrectly reuse prefix IDs. Discover all validation
targets over the complete union before partitioning: types, inverse edges,
required values and subclass relationships may occur in later chunks. Independent
per-chunk SHACL passes do not prove whole-graph conformance.

The completion receipt repeats the cut transaction, source, graph and profile
hash. Its output digest hashes successive big-endian unsigned 64-bit chunk
lengths and exact chunk bytes. The shared applier checks record counts and
per-graph digests before completion. Record decoding and RDF literal encoding
reuse the same implementations as replication and ordinary RDF export.
RDF export omits closed valid-time claims, while the scratch replica retains
them. Duplicate physical claims collapse in the RDF union. Unsupported RDF
values refuse rather than silently disappear.

This establishes a coherent input for a validator, **not** a SHACL verdict,
production memory bound, installed capability or permission to restore a cron.
