# Compose knowledge packs

`quipu compose` loads verified local share directories or archives into one
store, retaining each pack as a named graph and naming their union as a dataset.
ROOT remains unchanged, including when validation passes. Composition is an
inspection operation; it does not grant foreign content trust.

```bash
quipu compose ./operations-pack ./repository-pack \
  --shapes-from ./operations-pack --db composed.db > composition.json
```

The command returns JSON and exits 0 for an admitted union, 2 for a retained,
quarantined union, or 1 for a refusal. Exit 2 means the composition is available
for inspection in quarantine, not that no data was written. Integrity and
shape-selection errors refuse the whole operation. Internal shares require
explicit `--destination internal`, as on the import path.

## Identity and provenance

Exact IRIs denote one entity across packs. Matching labels never merge entities.
An explicit `owl:sameAs` assertion supplies an auditable identity link; consumers
can follow it while the original names remain in the source graphs. Blank node
identifiers are scoped to the pack even when independent exports both use `c14n0`.

The result names the dataset and each source graph. Query the dataset explicitly:

```sparql
SELECT ?item ?name FROM <urn:quipu:composition:HASH>
WHERE { ?item a <https://example.org/Item>; <https://example.org/name> ?name }
```

For a fact's source membership, name the source graphs from the result:

```sparql
SELECT ?pack
FROM NAMED <urn:quipu:composition:pack:FIRST_SHARE_HASH>
FROM NAMED <urn:quipu:composition:pack:SECOND_SHARE_HASH>
WHERE { GRAPH ?pack { <https://example.org/item> <https://example.org/name> "Item" } }
```

A fact supplied by both packs has two source memberships. The dataset's RDF union
suppresses duplicate triples. `urn:quipu:composition:metadata` records JSON manifests
under `urn:quipu:composition:manifest`, including source references, share and store
identities, transaction anchors, original manifests, attestation status, and validation
results. Records describe observed
compositions; they are not an automatically maintained validation certificate.

## Shape authority and partial snapshots

Without `--shapes-from`, every input must carry an identical shape bundle.
Different bundles are refused with their share IDs and hashes. Explicit selection
chooses that pack's complete bundle for this composition. It never silently unions
conflicting constraints or installs foreign shapes as global policy.

Validation examines the union: another pack can supply a required field. Loaded
local shapes control admission when present, including when an incoming bundle
declares a conflicting policy. Without local shapes, explicit `--shapes-from`
selection authorizes that bundle's `quipu:onViolation "emit"` policy. Default
identical bundles validate the union but do not implicitly authorize emit
admission. Missing policies mean reject; unknown or conflicting values refuse.

Admission parses policy by RDF subject, so compact Turtle, alternate prefixes,
comments and string literals cannot route a neighboring reject shape into emit.
Both validator graphs preserve all constraints and shared RDF-list dependencies;
only inactive independent targets are removed, including implicit class targets.
Policy on a nested property does not downgrade its enclosing reject shape.
The four core target kinds are supported; custom SHACL-AF targets refuse when
emit partitioning is requested. Canonical and legacy annotation IRIs are checked
together, so conflicting declarations cannot choose their own precedence.

`conforms` retains strict SHACL meaning: any diagnostic, including an advisory
Violation or Warning, makes it false. Admission uses `blocking` separately.
Emit-policy diagnostics appear in `advisory_results`; reject-policy Violations
still quarantine the union. The report records `admission_policy` with its source,
shape hash and emit authorization. The existing `shapes_authority` field identifies
the selected input bundle.

The report preserves violation counts and counts by source shape. Its `results`
preview contains at most 40 diagnostics; when truncated, `complete_results`
contains every diagnostic and `results_total` records their count. Types outside
the authoritative vocabulary are reported separately as `off_vocabulary` and
also quarantine the union. Neither an admitted union nor an emit policy promotes
data to ROOT or grants foreign content trust.

The snapshot vector remains visible. Different source stores' transaction anchors
are incomparable. Mixed dates are permitted and reported; composition does not
claim that snapshots were taken together, every referenced entity has arrived, or
missing facts represent upstream deletion. Pack selection is explicit. Loading a
different snapshot names another composition without replacing an earlier dataset.

## Retractions and replay

Reloading a share reuses its source graph rather than refilling it. Attested shares
still obey the ordinary signature and nonce-replay checks. A local
retraction in that graph survives reload of the same snapshot. Validation on replay
examines the current local union, so removing a required field changes the outcome
to quarantine. Source graph edits affect every dataset that includes that graph.

A distinct historical snapshot is a separate inspection graph, not a restoration
of ROOT or an automatic replacement of the selected dataset. Historical source
membership records what a snapshot contained, not whether its facts remain current.
Retract in the graph whose view you intend to change.

## A stable name per repository

Each composition gets a new dataset name, so an agent has nothing stable to
ask for "this repository's knowledge now". Repository mode gives it one:

```bash
quipu compose --repo scbrown/demo packs/main=./main.pendant packs/ops=./ops.pendant \
  --db store.db
```

Each input is `<pack-key>=<path>`. The pack key (its path in the repository, for
example) is what makes two commits of a pack the same pack, so it is supplied,
never inferred from the bytes. An admitted composition moves two datasets:

- `urn:quipu:repo:<repo>:current` names the latest admitted snapshot.
- `urn:quipu:repo:<repo>:previous` names the snapshot before it.

A quarantined composition stays inspectable under its own
`urn:quipu:repo:<repo>:snapshot:HASH` dataset, but it never moves CURRENT.
Recomposing the snapshot CURRENT already names moves nothing.

```sparql
SELECT ?item ?name FROM <urn:quipu:repo:scbrown/demo:current>
WHERE { ?item <https://example.org/name> ?name }
```

Pack graphs in this mode are scoped to the repository
(`urn:quipu:repo:<repo>:pack:<key>:SHARE_HASH`). Two repositories composing
identical bytes therefore get two graphs, and a retraction in one never shows
through the other. Older snapshot graphs that neither pointer names are dropped:
their facts are retracted and the graph is unregistered, and history stays
bitemporal. A graph that another dataset still names is kept and reported under
`retained_shared`.

**Local retractions carry forward.** Before each composition, statements
retracted locally in this repository's pack graphs are recorded as tombstones
for that pack key. They are removed from every later snapshot of the pack before
validation, so validation sees the union an agent will actually query. This
holds across a version where the statement is absent, a version where it
returns, retention pruning and rollback. Blank nodes are renamed per snapshot,
so a retraction of a blank-node statement cannot be matched later. That case
refuses the composition and names the statement; re-assert it, or give the node
an IRI in the pack.

Roll back by moving the pointer:

```bash
quipu compose --repo scbrown/demo --rollback --db store.db
```

This swaps CURRENT and previous, and first applies any tombstone recorded since
previous was current. The whole operation is one transaction: a failure at any
step leaves both pointers, the graphs and the caches as they were.

### Undo a local retraction

Revoke one carried-forward suppression explicitly, without editing SQLite:

```bash
quipu compose --repo scbrown/demo --pack-key packs/main \
  --revoke-tombstone '<https://example.org/alice> <https://example.org/name> "Alice" .' \
  --reason 'Undo an accidental local retraction' --db store.db
```

The statement must be exactly one N-Triples triple, with no blank nodes. A
nonblank reason is required. This operation takes no packs and cannot be combined
with rollback. It restores only matching, previously asserted composition facts
that are retracted in this repository's owned graphs for the exact pack key,
then removes their carried-forward tombstone in the same transaction. The
receipt reports `revoked` and `graphs_restored`. Repeating a successful reversal
returns `revoked: false`; a missing statement never invents a fact.

**Replaying or rolling back an already-stripped graph stays stripped.** A
tombstone may have removed the statement before that graph was first stored, so
there is no historical assertion there to restore. Recovery is a fresh
composition from a newly published source-pendant snapshot containing the fact
(a distinct share ID), after revocation. Replaying the same stripped snapshot
does not refill its graph. The operation never fabricates omitted facts from the
requested statement alone.

CURRENT and PREVIOUS do not move, but restored facts become visible immediately
in their graphs, including retained older graphs. Any other dataset referencing
those same graphs sees the restoration too. Other repositories, other pack keys,
unrelated statements and ROOT are unchanged. Recomposition and rollback do not
recreate the revoked suppression; a later explicit local retraction can create
it again. A failed operation rolls back both restoration and the ledger update.

Deleting a tombstone row by hand does not have these semantics: retracted facts
in a retained graph would be harvested again on the next composition.

## Pack pointers on first use

A manifest's `kind = "pack"` pointers remain declarations at checkout and
ordinary composition. A consumer follows only the graph it needs, using
`share_pointer_load::LazyPacks::use_graph` or the native first-use command:

```sh
quipu compose --use-pointer urn:project:api \
  --declaration repository.pendant.tar.gz \
  --authority local-policy.pendant.tar.gz \
  --repo-remote https://github.com/example/project.git \
  --pointer-cache ./verified-archives --db ./inspection.db
```

The receiver supplies the repository remote and local authority. The command
runs `caboodle packs trust check` against that repository and the pointed
archive hash before reading the cache or fetching. A missing trust tool or a
refused decision stops the operation. Foreign bundles never become global
shape policy, and nothing is promoted into ROOT. Use the returned `dataset` in
an explicit SPARQL `FROM` clause. This command does not move repository CURRENT.

The archive sha256 and the enclosed manifest's `share_id`, graph, shapes and
query hashes are verified independently. Cache keys use the archive digest;
reads reverify the bytes and recheck trust. Cached archives work offline. A
corrupt cache entry refuses rather than silently fetching a replacement.
Fetch, integrity and validation failures name the requested pointer and leave
no partial composition. Nested pointers are reported, not recursively loaded.
The download limit is 16 MiB and the expanded archive limit is 64 MiB.

Library consumers supply their own trust and transport callbacks plus an
`ArchiveCache`. Construction performs no network access. Invoke `use_graph`
when a graph is first requested, rather than scanning and following every
pointer. `DirectoryCache` provides atomic native archive writes.

### Browser consumers

The explorer wasm bundle exports `PointerExplorer`. Supply the declaration
archive and an authority archive chosen by the receiving application. The
separate `wasm/explorer/runtime/pointers.mjs` helper provides bounded Fetch and optional
CacheStorage persistence:

```js
const engine = new PointerExplorer(declarationBytes, localAuthorityBytes);
const cache = await caches.open('project-pointer-archives');
const result = await usePointerGraph(engine, 'urn:project:api', authorize, { cache });
const rows = engine.query(`SELECT ?s ?p ?o FROM <${result.dataset}> WHERE { ?s ?p ?o }`);
```

`authorize(pointer)` is the receiving application's policy decision, such as a
Caboodle policy exported by its trusted host. It must return the literal
boolean `true`; the archive cannot supply consent. The helper checks it before
cache or network access. Rust checks trust and both integrity layers before
composition, using the same implementation as native. Browser requests omit
credentials and refuse redirects. Cached archives can be used by a fresh
instance offline. This is a read-only named dataset; it does not use the
ordinary explorer's separate ROOT promotion ceremony.
