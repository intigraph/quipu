# Named Graphs, Overlays & Datasets

> **Implementation status:** ✅ **Built** on the sanctioned surfaces — the `g`
> column and `graphs` registry, overlay create/write/compose, `GRAPH` /
> `FROM` / `FROM NAMED` evaluation, the `graph` query param, and named
> datasets, and (since 2026-08-21) a strict `graph` param on `POST /knot`
> that accepts only already-registered committed-class graphs. Deliberately
> **not** built: property paths under `GRAPH ?g` (explicitly refused, never
> a silent ROOT read).
> See `docs/design/named-graphs.md` for the full design.

Every fact in quipu's EAVT store carries a graph coordinate on top of
`(entity, attribute, value)` and the two time axes — the store is really a
quad store. The `g` column says *which* subgraph a fact lives in, orthogonal
to *when it holds* (`valid_from`/`valid_to`) and *when the store learned it*
(`tx`). Retraction, time-travel, and contradiction detection all scope
within a graph: a retraction in graph A never touches graph B.

- `g = 0` is the reserved **ROOT** graph — the default committed graph, the
  source of truth.
- A named graph's `g` is the interned term id of its graph IRI, so resolving
  `GRAPH <iri>` is a single term lookup.
- The `graphs` registry keeps one row per graph with an enforced `class`:
  `committed` (a durable branch; ROOT is the seeded, self-rooted one) or
  `overlay` (a layer over a committed parent). A graph's class is fixed at
  create.

## Querying: `GRAPH`, `FROM`, and the `graph` param

**SPARQL reads are ROOT-scoped by default.** The default graph is ROOT
alone, not an all-graph union — silence must never expose another tenant's
overlay. A query widens its dataset only by saying so:

- `GRAPH <iri> { … }` scopes the enclosed patterns to one named graph. An
  unknown IRI matches nothing.
- ROOT is the default graph, not a named graph. `GRAPH <urn:quipu:graph:root>`
  is refused with an error naming the default-graph form. Read ROOT without a
  `GRAPH` clause, or explicitly use `FROM <urn:quipu:graph:root>`.
- `GRAPH ?g { … }` ranges over the active named graphs, binding `?g` to each
  match's graph IRI. Property paths under `GRAPH ?g` are refused with an
  explicit error rather than silently reading ROOT.
- `FROM <g…>` makes the default graph the RDF merge (union) of those graphs.
  An unknown graph contributes nothing; an all-unknown `FROM` set yields an
  **empty** default graph — never a fall-through to ROOT.
- `FROM NAMED <g…>` restricts which named graphs a `GRAPH` clause can see. A
  query with `FROM` but no `FROM NAMED` activates no named graphs (per
  SPARQL 1.1), so `GRAPH` matches nothing.

```sparql
SELECT ?s ?title
FROM <http://example.org/graphs/derived>
WHERE { ?s <http://example.org/title> ?title }
```

`/set` and `/retract` operate only on ROOT and leave named-graph facts intact.
They refuse a supplied `graph` field, including `null`, before changing facts.
For a named graph, use the explicitly scoped `/update` endpoint instead.

`POST /query` and the `quipu_query` MCP tool also take a `graph` request
param — a convenience that scopes the query's *default* graph to one named
graph without writing a `FROM` or `GRAPH` clause:

```json
{"query": "SELECT ?s ?o WHERE { ?s <http://example.org/p> ?o }",
 "graph": "http://example.org/graphs/derived"}
```

Omitting it keeps the ROOT default; an unknown IRI gives an empty default
graph; a `FROM` clause in the query text overrides the param. The param also
resolves **dataset names**: passing a dataset IRI scopes the query to that
dataset's members, so `FROM <dataset>` and `"graph": "<dataset>"` mean the
same thing. The same `graph` param on `quipu_export` / `POST /export`
exports one named graph's facts instead of ROOT.

### Search has a broader default than SPARQL

On a new store, `POST /search` and `quipu_search` default to ROOT plus registered
named graphs. Existing stores retain ROOT until explicitly broadened with
`[quipu.search] default_graph_scope = "all"`. When that setting is omitted, the
store persists its default; read-only legacy stores use ROOT without migration.
Coverage reports the effective default and its source. This policy is local to
the receiving store: importing a share does not replace it with the sender's
choice. A whole-file backup retains the store's policy.
For example, a work tracker can keep its records in its own project graph:
those records need not be copied into ROOT to be discoverable. This covers
graphs registered in the local store, including attached layers registered
there; it does not query remote federation endpoints automatically.

Configure retrieval policy under `[quipu.search]`:

```toml
default_graph_scope = "all"
exclude_graphs = ["urn:example:quarantine", "urn:example:private-work"]
```

Use `default_graph_scope = "root"` for the previous implicit scope. Requests
can select `graph: "urn:quipu:graph:root"`, one named graph IRI, `graphs: [iri,
iri]`, or `all_graphs: true`. Configured exclusions apply to all selections;
a selection emptied by exclusions refuses. These controls are retrieval
policy, **not access control**. Deployments needing tenant isolation must
enforce authorization separately. Configure exclusions before exposing a
store containing scratch or quarantine graphs to general search.

Every successful search returns `search_scope`: selected graph IRIs,
configured exclusions, whether selection was explicit, named-vector enablement,
and the fact that index completeness is not certified. Result graph provenance
does not promote a fact or make its source authoritative. Similarity and context
admission still apply; an empty search does not prove a graph contains no facts.

Semantic retrieval uses one vector per entity. An entity with ROOT text keeps
that representation; a named-only entity combines its named-graph facts.
Selecting a graph filters membership, not the vector's text provenance. For
assertion-level literal matching within one graph, use keyword search. This
distinction is why listing selected graphs does not certify complete indexing.

Semantic search across named graphs still requires `named_graphs = true`.
Prepare existing named-only vectors using bounded `/embed_backfill_graph` calls
before enabling it; an unprepared default all-graph request refuses instead of
silently reporting ROOT as complete coverage. Keyword search needs its enabled,
backfilled lexical index, but no embedding provider or named-vector flag.
ROOT-only content ranking, anchor traversal, and structured search refuse a
broader scope. See the [search API](../reference/rest-api.md#post-search).

### Querying and promotion are separate operations

To inspect one graph, use `GRAPH <iri>` or the `/query` `graph` parameter above.
To join graph contents deliberately, use `FROM <a> FROM <b>` or a registered
dataset. Search scope never changes these SPARQL defaults or writes facts.

Promotion admits reviewed content into ROOT; it is not required merely to find
or query named-graph data. For bulk material, use the producer's `share`, then
`import` to stage and validate it, inspect the admission result, and use
`import promote <share-id>` only when eligible and authorized. Do not replace
promotion with a loop of episode or update writes. See
[bulk loads and migrations](../recipes/bulk-loads.md) for exact commands,
shape admission, and fork rollback. A search hit from a staging graph is still
staged content until that governed promotion succeeds.

Property paths follow a fixed graph scope without crossing it: a
`GRAPH <iri>` closure stays inside that graph, a `FROM <a> FROM <b>` path
traverses their merge. A path never crosses a graph boundary — half a path
in an overlay and half in ROOT is not a fact either graph asserts.

## Overlays

An overlay is a scratch layer over a committed parent branch: hypotheses go
in the overlay, the committed base is never mutated. Two write primitives,
one uniform read:

- **Create** (`quipu_overlay_create` / `POST /overlay/create`) registers an
  overlay-class graph **bound once** to its committed parent branch (ROOT by
  default). Idempotent; rebinding to a different parent is an error — the
  binding is unforgeable.
- **Write** (`quipu_overlay_write` / `POST /overlay/write`) takes one of
  three ops: `assert` and `retract` are graph-scoped writes into the
  overlay; `tombstone` marks a specific `(e, a, v)` from the parent as
  *absent* in the overlay's composed view, without touching the parent.
- **Compose** (`quipu_overlay_compose` / `POST /overlay/compose`) resolves
  the stack `[overlay > parent-branch-root]` with a single rule: a triple is
  present iff **asserted and not tombstoned**, nearest-overlay-wins. Overlay
  asserts shadow the parent; overlay tombstones hide parent triples;
  everything else falls through.
- **Governed precedence** (`"precedence": "governed"` on the same tool /
  endpoint) inverts who wins, for reading a **quarantine plane** over a
  governed graph (quipu-e61, adopted from Spanner's
  statically-defined-properties-win-over-dynamic rule): the parent's facts
  are always present — an overlay value on a same-subject-same-predicate
  slot the parent claims is suppressed, and an overlay tombstone cannot mask
  a parent fact (it still masks the overlay's own contributions). A
  low-trust plane may *extend* the governed graph but never alter what it
  says; promotion out of quarantine remains an explicit governed write,
  never a precedence flip.

Many tenants can extend the same base independently this way, and a
committed read never sees any of them unless the query names the overlay.
Overlays are one sanctioned write path for named graphs. `POST /episode`
accepts a `graph` field for ingestion into a named graph, and `POST /knot`
accepts a `graph` field under a strict contract: the target must already be
a **registered committed-class** graph (created via `graph_create`, where
authority checks live). An unknown IRI is an error and is never interned —
a typo'd plane name must not become a writable target as a side effect of
being rejected — and overlay-class graphs are refused (write through
`overlay_write`). The class invariant the earlier "no `graph` on `/knot`"
refusal protected survives because registration remains the only way to
mint a target. The semantic event taxonomy (`episode.ingested`, `entity.*`,
`edge.*`, `type.new`, `predicate.new`) emits for ROOT-graph commits only;
named-graph writes — overlay staging and committed-class targets alike — do
not emit it. Gate refusals, however, are recorded as `write.refused` events
for **any** destination graph (the payload names the graph IRI), and
registry changes (`shapes.loaded`, `fork.*`) emit their own events.

## Datasets

A dataset is a **name for an arbitrary set of graphs**, queryable as one
unit — the reusable form of a `FROM a b c` clause, so a graph set can be
labelled, governed, and handed to another agent. Managed via the
`quipu_datasets` MCP tool or `POST /datasets` (create / list / show /
remove).

```bash
curl -s localhost:3030/datasets -X POST \
  -H "Content-Type: application/json" \
  -d '{"action": "create", "name": "http://example.org/datasets/hot",
       "members": ["http://example.org/graphs/a", "http://example.org/graphs/b"]}'
```

- `FROM <dataset-iri>` (and the `graph` query param) expands to the
  dataset's members at resolve time; everything downstream reads the
  expanded set.
- A dataset is never implicitly active — the ROOT-alone default is
  untouched; you get a dataset's graphs only by naming it. Creating a dataset
  with a member that names no registered graph is refused, and the refusal
  names that member.
- `urn:quipu:graph:root` is a valid member and reads the default graph, the
  same as `FROM <urn:quipu:graph:root>`. A dataset of ROOT plus a named graph
  is how a stored query reads the default graph AND that graph, without
  hardcoding either in the query text.
- Members may carry a declared ordering (`{"graph": …, "ord": N}`);
  duplicate ranks are refused rather than tiebroken silently. An empty
  dataset is refused.
- Datasets are mirrored into the meta-graph as `quipu:Dataset` /
  `quipu:includesGraph` facts, so they are queryable and governed like any
  other fact. Datasets are orthogonal to the overlay branch tree: the branch
  tree is compose's resolution root, datasets are overlapping named sets.
- The labels of a query's active dataset compose across its member graphs —
  see [Graph Labels](./graph-labels.md) for how freshness/trust/policy
  labels fold over the graphs a query actually reads.

## Related

- [REST API](../reference/rest-api.md) — `/query`, `/export`, `/overlay/*`,
  `/datasets`
- [MCP Tools](../reference/mcp-tools.md) — `quipu_query`, `quipu_export`,
  `quipu_overlay_*`, `quipu_datasets`
- [Graph Labels](./graph-labels.md) — labels on graphs and their composition
  over datasets
- Design doc: `docs/design/named-graphs.md`
