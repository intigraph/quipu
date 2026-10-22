# Atomic source acquisition

`POST /acquire` returns complete canonical N-Triples for explicitly selected
named graphs, plus a separate `quipu-acquisition/v1` source receipt. A consumer
must recognize that contract in `/version.read_contracts` before using it.
Historical acquisition through `/share` is unsupported.

```json
{
  "source_store_id": "the-known-source-lineage-id",
  "graphs": [
    "https://example.org/project/board"
  ],
  "tx": 42,
  "max_bytes": 262144
}
```

The expected store identity and exact required graph IRIs come from
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

## Composed service acquisition

`POST /acquire/composed` uses the separate
`quipu-acquisition/composed-v1` contract. `/acquire` retains its blanket
attachment refusal. Consumers must gate on the new `/version.read_contracts`
entry and retain the returned source receipt unchanged.

This first composed profile selects only registered, committed MAIN graphs.
Other providers remain mounted and appear in a complete, bounded census, with
`outside-qualified-main-scope` exclusions. Required unknown or attached graphs,
dataset aliases, overlays, frozen selections, overlapping graph ownership, and
selected identities affected by composed term aliases refuse. The direct-main
reader explicitly differs from composed `/query` resolution: its advertised
reader is `qualified-main-committed/v1`. The ownership digest binds the full
selected graph label cache and configured floor. This first profile supports
cache-only, timeless label-floor evaluation; expiring labels or trust-derived
floor inputs refuse rather than borrowing unbound meta-graph history.

The request adds `expected_generation` to the v1 fields. It is optional for
present capture, but required with any explicit `tx`: capture the admitted
generation first, then submit that token with the MAIN pin. Equal transaction
integers cannot identify a composition. The response
contains `graphs`, `source_receipt`, and `source_receipt_sha256`. The receipt
contains its own `schema`, `complete`, a nested `main` receipt with schema
`quipu-acquisition/main-direct-v1`, and `composition`. The main receipt's
`applied_tx` applies only to MAIN. Composition binds the connection incarnation,
selected ownership and policy digest, and every mounted provider's alias,
lineage, opaque physical identity, byte hash, manifest hash, local head, term
space, graph inventory, and exclusion. It is not a global transaction clock.

The server serializes topology access on its canonical store handle and holds
one MAIN read transaction. On Linux x86-64 and AArch64, read leases retain the
provider files during census, hashing, selected reads, and final checks.
The lease fence also refuses an existing SIGIO handler or blocked/pending
notification on its synchronous thread. Existing writers, a lease break,
mutable journal content, replacement, or a
change in physical file identity refuse. Read-only mount configuration alone
is insufficient. Other platforms refuse attached composed acquisition while
still allowing the attachment-free direct-main profile.

Historical admission starts at the connection's first successful observation
of that scope and fenced topology. The composition's `history_min_tx` records
this floor separately from retained MAIN transaction history. Changing topology
or policy resets the floor; restarting creates a new incarnation. Runtime
mount, unmount, or synchronized remount invalidates the incarnation before the
transition, even if the same file returns or a transition fails. An old MAIN
pin cannot retroactively use a newly attached pack. A previously observed pin
can be reused under its unchanged generation after ordinary MAIN data writes.
An expected-generation mismatch refuses rather than silently resealing data.

Limits are 16 selected graphs, 16 mounted providers, 512 named graphs per
provider, 256 MiB of aggregate provider identity reads, 100,000 selected rows
per graph, an 8 MiB response, and a ten-second operation ceiling. A shorter
request deadline also applies. Caller `max_bytes` only lowers the response
ceiling; graph bytes consume one aggregate budget during reading rather than
accumulating a separate full allowance per graph. Unsupported RDF values, including raw byte values, refuse. No error
returns a complete receipt or falls back to an empty or present-state export.

Profiles must name their actual required scope. Committed comments may reside
in the project graph itself; do not invent a separate comments graph. A
committed-project profile that excludes derived ephemerals does not satisfy a
full SDK snapshot requiring them. Unknown required sidegraphs refuse. Producer
source tests and a version advertisement do not establish configured adapter
acceptance, installed publication, or runtime activation.

## Eligibility observation

`POST /acquire/eligibility` (contract `quipu-eligibility/v1`) reports, for 1 to
16 exact named graphs, whether each would pass direct-MAIN acquisition admission
and the store's configured label floor **at one observed head**:

```json
{"source_store_id": "urn:uuid:...", "graphs": ["https://example.org/project/board"],
 "expected_head_tx": 4210}
```

The response carries `observed_head_tx`, a sanitized `policy` identity (a
SHA-256 of the floor and the names of the configured axes, never their values),
the producer identity, and per graph `eligible`, `refused` (scope admission or
policy, with the reason) or `unknown` (the check failed; never eligibility).
`expected_head_tx`, when given, refuses unless the head is exactly that one, and
the head and policy are re-read after the checks; a change refuses.

It reads graph metadata, terms, the dataset registry, transaction ids and store
identity in one read transaction, and nothing else: no facts, no writes. It
holds the writer connection through a dedicated guard that never settles a
signed attestation and never synchronizes or remounts providers, and it refuses
a signed request outright. A configured trust- or time-derived floor refuses as
unsupported before any label read, exactly as acquisition does.

An observation is **not** an acquisition receipt, an applied-pin policy proof,
or authority to acquire. It carries no `applied_tx`, and a later acquisition
can still refuse.

## Physical scope measurement

`quipu acquire scope` (contract `quipu-acquisition-scope/v1`) measures, for 1
to 16 exact named graphs at one applied pin, the physical `DISTINCT(e,a,v)`
rows a direct-MAIN acquisition reads, using the acquisition's own row
predicate, and the bytes of their values:

```sh
quipu acquire scope --db artifact.db --source-store-id urn:uuid:... \
  --graph https://example.org/project/board --tx 4210 \
  --max-vm-steps 50000000 --deadline-ms 60000
```

Each graph is first admitted exactly as acquisition admits it; a refused graph
is reported and not read. A graph with more than 100,000 rows is reported as
`exceeds_cap` with no count. The VM-step and wall budgets are required: SQLite
counts steps in strides of 1,000 per statement, and exhausting either budget
aborts the whole run with the phase that was running and no partial result.

The store is opened read-only with no migration, and a missing store is never
created. The measurement returns counts only, never fact content. Running it
against a production store or a copy of one is a read-cost decision for that
store's owner; the command does not authorize itself.
