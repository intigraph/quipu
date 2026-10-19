# Versioned file memory

File memory stores exact UTF-8 source text separately from extracted knowledge.
It preserves BOMs, line endings and links. An owner is an exact deployment,
project and crew IRI tuple. Names, account labels and caller actor fields do not
establish authenticated ownership.

The feature is disabled by default. A deployment must register the graph and
set `[quipu.memory_files]` with `enabled = true`, `graph`, `deployment` and
`project`. No graph or credential is created by these endpoints. Existing graph
authority, vocabulary, SHACL and write-policy admission still apply.

## HTTP operations

All operations accept JSON and return bounded JSON envelopes:

- `POST /memory/read`: owner tuple; current immutable manifest or null, with
  `coverage = "this_repository_owner_records"`.
- `POST /memory/export`: owner tuple; manifest and exact identified file
  revisions. Cross-crew reads retain the deployment's inherited read boundary.
- `POST /memory/projection`: owner tuple; same export plus
  `authenticated_owner`. Requires an existing verified named credential whose
  principal exactly matches the owner. A shared bearer cannot prove this.
  This endpoint reads data; it does not write native files.
- `POST /memory/receipt`: the exact original intent; returns its verified receipt
  or null without retrying the write. Requires the matching named principal.
- `POST /memory/commit`: one file intent with owner, relative path, UTF-8 `text`,
  expected snapshot and file revision, idempotency key, harness and opaque account
  label. A null text is an explicit tombstone. Requires the matching named
  credential and the normal writable-server policy. A stale revision returns
  HTTP 409 without changing the owner head.

Clients must retain the exact intent until its receipt is verified. Repeating
that intent and idempotency key returns its original receipt even after the
head advances. Reusing a key with different content refuses. A new stale intent
conflicts even when its bytes equal the current file. Timeouts are indeterminate;
clients must not silently create a new key or replace local files.

An owner snapshot contains at most 128 file entries and 4 MiB of live text; each
file is at most 256 KiB. Tombstones remain in the manifest. Invalid UTF-8,
traversal paths and ambiguous stored records refuse. The complete manifest is
conditional on the owner head inside the ordinary transaction savepoint.

Generic RDF mutations cannot edit reserved file-memory subjects or predicates,
even when caller-supplied actor/source fields resemble a memory writer. The
shared transaction pipeline requires a private capability for the validated
protocol's exact rows. This also protects retractions and duplicate source
claims. Current memory records must be changed through the memory protocol;
privileged offline database restore remains filesystem authority.

Records use the existing base `Artifact` class with a memory profile, rather
than minting new ontology classes. Owner, revision, snapshot and receipt records
carry distinct kinds and explicit owner/project/deployment references. Their
source JSON is versioned data, not an extracted factual assertion. The public
repository release selector excludes memory subjects, predicates and references,
including memory attached to an otherwise public subject. This export boundary
does not introduce per-crew read secrecy.

## Local CLI workflow

The local CLI uses the same core, but its authority is the filesystem principal
that can open the database. It cannot isolate crews from a user who can modify
that database. Every invocation requires an explicit database and graph:

```sh
qp memory read owner.json --db fixture.db --graph urn:example:memory
qp memory commit request.json --db fixture.db --graph urn:example:memory --local-authority
qp memory receipt request.json --db fixture.db --graph urn:example:memory
qp memory export owner.json --db fixture.db --graph urn:example:memory
```

The graph must already be registered. `owner.json` contains the three exact IRI
fields. A first request uses null `expected_snapshot` and `expected_file`; later
requests use the returned snapshot and file revision. The `account` field is an
opaque selected-account identifier, never a credential. Help opens no store.

This export is a file-data envelope, not the older RDF export/share/import
contract and not an instruction to overwrite a native memory directory. RDF
`group_id` selection denotes provenance, not owner authorization.

## Filesystem and harness guarantees

A consumer must verify authenticated own identity, manifest coverage, each
revision ID, owner, path, byte count and SHA-256 before staging native data.
A safe generic consumer creates an exclusive retained generation. A lock or a
last hash check cannot make replacement safe against a noncooperating editor.
Native root selection is a separate harness-specific operation; staging alone
does not prove a running model consumed the files. Unsupported adapters must
remain explicit and preserve the previous memory bridge.
