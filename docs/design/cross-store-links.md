# Links and queries across graphs, stores and hosts

Status: proposed design, not implementation. It adds no default behaviour, route,
credential or production authority.

Named graphs are moving apart. [Per-graph writable stores](per-graph-writable-stores.md)
put routed graphs in separate SQLite files on one host;
[local sync mode](local-sync-mode.md) and [replica composition](replica-composition.md)
put slices of graphs on other hosts; peers may hold graphs the server has never
promoted. This document answers one question for all three cases: **how does an
entity in one graph refer to an entity in another, and how does a query follow
that reference?**

It is a decision layer over existing designs, not a replacement for them. Where a
sibling design already decides something, this document cites it and does not
restate the mechanism.

## Summary of decisions

1. **A link is an ordinary triple.** It is stored in the graph of the party that
   asserts it, with the other entity's IRI as its object. There is no link table,
   foreign key, proxy node or stub copy of the target.
2. **IRIs are identity; aliases are claims.** Two IRIs denote the same entity only
   through a decided row in an alignment graph
   ([cross-graph alignment](cross-graph-alignment.md)). Queries follow such aliases
   only when asked to, and only from decided rows.
3. **A query names every graph it reads.** Following a link never adds the target
   graph to the dataset implicitly. A link into a graph outside the authorized,
   declared dataset is unresolved, not empty.
4. **Composition is local; federation is explicit.** Graphs on one host compose
   through the [multi-DB composition](multi-db-composition.md) and per-graph store
   readers. Graphs on another host are read only through `SERVICE` or a pinned
   as-of copy. There is no transparent distributed join.
5. **Every answer carries its cut.** A result that reads more than one store reports
   the coordinate `(store_id, log_epoch, tx)` and state of every graph it read, plus
   a `complete` flag. No scalar transaction ID crosses a store boundary.
6. **Trust never rises along a link.** A row is no more trusted than the least
   trusted graph that produced it. A link from a trusted graph into a quarantined
   peer graph does not promote anything in the peer graph.
7. **Validation never reaches past declared context.** A shape that depends on a
   neighbour in another store validates against a declared, bounded, pinned
   context, or its result is `UNKNOWN`. It never passes because the neighbour was
   not visible.

## 1. Link model

### What a cross-graph edge is

A cross-graph edge is `<s> <p> <o>` written into graph `G`, where `<o>` is an
entity whose descriptive facts live in another graph `H`. Nothing in the triple
records `H`. This is already how the store behaves: term IDs are local to a file
and never compared across files, and IRIs are the identity
([per-graph writable stores](per-graph-writable-stores.md), "IRIs stay the
identity").

The edge lives in the graph of the **asserting party**, which is the graph whose
owner is entitled to say it. In practice that is the subject's graph: a code
projection that says a commit references a work item writes that triple in the
code graph, not in the work-item graph. Consequences:

- Writing a link needs write authority over `G` only. It does not need authority
  over `H`, and it does not change `H`'s share hash, replication stream or
  provenance.
- Dropping or replacing `G` removes its outgoing links and nothing else.
- `H` cannot see who points at it without reading the graphs that point at it.
  Inbound queries are reads over the linking graphs, under the same grants.

An IRI link is **not** a foreign key, an ownership grant or a security tenant
(per-graph writable stores, "Constraints"). A link to an IRI that has no facts in
any readable graph is a valid triple. Whether that is a defect is a validation
question (section 5), not a storage one.

### Rejected alternatives

| Alternative | Why not |
|---|---|
| A central link table or link graph for all cross-graph edges | It becomes a writer every producer needs, breaks per-graph write isolation and puts ownership of the assertion in the wrong place. |
| Proxy or stub nodes in `G` that copy the target's label/type | Copies go stale without a refresh contract, and they make `G` assert facts about entities it does not own. |
| Rewriting the object IRI on import to a local IRI | It loses identity; [cross-graph alignment](cross-graph-alignment.md) already requires import to stop rewriting IRIs. |
| Storing the target graph IRI on the edge (`<s> <p> <o>` plus `quipu:inGraph <H>`) | Graph membership of an entity is not a property of the edge, can be plural and changes over time; it belongs to the catalog (below). |

### Identity across different IRIs

When two graphs mint different IRIs for the same thing, the link is an alignment
row, not an edge. The existing alignment design applies unchanged:

- The SSSOM mapping set is stored as RDF in its own committed graph
  (`urn:quipu:align:<a>:<b>`), never written into either source graph.
- `owl:sameAs` is derived only from decided, non-negated `sameAs` rows.
- Broad, narrow and close matches are evidence, never identity.

The missing piece is the reader. Today nothing in query evaluation follows
`owl:sameAs`, so asserted aliases are inert. This design adds an **opt-in alias
mode** to a query, not a global inference:

```json
{"query": "...", "from": ["G", "H"], "aliases": {"graph": "urn:quipu:align:G:H", "grade": "decided"}}
```

- The alignment graph must itself be in the authorized dataset.
- Only decided `sameAs` rows are followed; there is no transitive closure beyond
  what the alignment graph states (the alignment design forbids chains).
- A result row produced through an alias is stamped with the alignment row that
  justified it, so a revoked mapping is traceable.

### Discovering which graphs a graph links to

A query planner, an authorization check and a validation context all need to know
which graphs `G` depends on. That is a **derived** fact, not an assertion:
the catalog may record, per graph and generation, the set of other registered
graphs that hold facts about the objects of `G`'s edges. It is recomputable,
never authoritative and never used to grant access. It exists so that an
operator can see "the code graph links into the work-item graph" before moving
either of them, and so that the first pilot can report dangling links (build
step F1). Because it names which graphs hold facts about which IRIs, the derived
set is operator-only catalog data; readers see only the grant-limited audit
described under "Link audit visibility".

## 2. Query composition

There are three cases. They differ in what can be joined, not in the link model.

| Case | Where the graphs are | Join | Mechanism |
|---|---|---|---|
| Same store | Named graphs in one file | Full SPARQL | Existing evaluation |
| Same host | Separate files (per-graph stores, attachments) | Full SPARQL over a strict common cut | Composed reader over `facts_source()`; IRIs resolved per file |
| Other host | Replica, peer, remote server | Bounded: local join of remote rows | `SERVICE` to a configured endpoint, or a local pinned as-of copy |

### Same host

The per-graph store design and the ingest pilot already define this read: the
query names its graphs with explicit `FROM`/`FROM NAMED`, the reader captures a
common cut over the selected files, a selected unavailable graph refuses the read,
and attached-file count is bounded. Following a link is an ordinary join, because
both sides resolve the same IRI in their own file. Term IDs are never compared
across files; the composed reader compares terms.

Nothing new is needed for correctness. What this design adds is the reporting in
section 3 and the dangling-link diagnostic in F1.

### Other host

`SERVICE` today sends `SELECT * WHERE { inner }` to an exactly configured endpoint,
evaluates it once and joins the rows locally. It does not push outer bindings to
the remote, so a link-following query such as

```sparql
SELECT ?item ?title WHERE {
  GRAPH <code> { ?commit <references> ?item }
  SERVICE <https://work.example/sparql> { ?item <title> ?title }
}
```

pulls every `title` from the remote. The smallest useful change is a **bounded
bind join**: when the left side has produced at most `N` distinct bindings for the
variables shared with the `SERVICE` block, send them as a `VALUES` block in the
remote query. If the left side exceeds `N`, refuse with a message that names the
limit. Do not silently fall back to the unbounded pull, and do not build a
cost-based distributed planner.

A bind join **discloses local data to the remote**: the IRIs and literals in the
`VALUES` block are whatever the local graphs produced, including values from graphs
the remote operator could never read. So pushdown is governed by a disclosure rule,
not only by the size limit:

- Each configured endpoint declares the local graphs whose terms may be sent to it
  (`disclose_from`). The default is empty, which means no pushdown: the endpoint
  gets the unbound inner query, exactly as today.
- Before sending, the evaluator traces every pushed-down binding to the graphs that
  produced it. If any contributing graph is not in `disclose_from`, the query is
  **refused**. It does not strip the offending values or fall back to an unbound
  pull, because either would silently change the answer.
- Constants written in the query text are the caller's own disclosure and are sent
  as today.

F5's acceptance therefore includes a negative control: a binding produced from a
graph outside `disclose_from` makes the query refuse, and a capturing test endpoint
records that it received **no request at all**.

The provider seam (`POST /query` with `federated: true`) stays what it is:
whole-query fan-out with concatenated, tagged results. It is not a join engine and
this design does not make it one.

For data that must be joined repeatedly, the answer is a **pinned copy**, not a
better remote join: take an [as-of share](as-of-share.md) of the remote graph into
a local, read-only import overlay and compose it locally like any other graph. The
copy carries the remote's `{store_id, log_epoch, watermark_tx}` and per-graph
digests, so its age is explicit.

### Authorization

A reader's dataset is exactly its grant ([graph read authorization](graph-read-authorization.md)).
Links do not widen it:

- If `G` is granted and `H` is not, a query over `G` returns `G`'s edges with their
  object IRIs and nothing from `H`. An IRI is not a secret; `H`'s facts are.
- A query that names `H` without a grant for it is refused as a whole, as the
  read-authorization design already requires for any denied member.
- An alias mode needs a grant for the alignment graph as well.
- `SERVICE` is denied under the scoped-read profile today. That stays true until a
  per-endpoint grant exists; this design does not add one.

## 3. Consistency and time

A link points at an IRI, not at a version of the target. Whether the target is
current, stale or absent is a property of the read, so the read reports it.

### The cut is a vector

A composed or federated result carries a `cut` object: for each graph read, the
coordinate `(store_id, log_epoch, tx)` it was read at and its source (local file,
attachment, pinned copy, remote endpoint). For `SERVICE` results the coordinate is
whatever the remote reports; if it reports none, the coordinate is `unknown` and
the row is stamped accordingly.

Two read modes, matching the per-graph store design:

- **Strict** (default): every selected local graph is read at a common published
  cut. A selected graph that is unavailable refuses the read.
- **Vector**, opt-in: each graph is read at its own head and the result says so.
  Callers that accept a vector cut must not present the result as a single
  snapshot.

### Stale, absent or diverged targets

| State of `H` | Result |
|---|---|
| Present at the cut | Normal join |
| Selected but unavailable | Strict mode refuses; vector mode refuses for local graphs and stamps remote rows `incomplete` |
| Present, but a pinned copy older than the caller's freshness floor | Refused with the copy's age, using the existing `_freshness` label floor |
| Not selected | Not read; the edge's object is returned as an IRI only |
| `store_id` or `log_epoch` differs from the pinned copy's | The copy is from another lineage; refuse rather than join across lineages |

SPARQL itself cannot carry this distinction. An `OPTIONAL` over a graph that was
not read returns the variable unbound, exactly as it does when the graph was read
and has no match. Per-row tri-state values would mean changing SPARQL semantics,
and this design does not attempt that. The mechanism is at the **answer** level:

- The `cut` lists every graph the answer actually read and its state (`present`,
  `unavailable`, `stale`, `lineage-mismatch`). A graph that is not in the `cut` was
  not read.
- `complete: true` is returned only when every graph the query names is `present`
  at the cut. In strict mode, anything else refuses. In vector mode it returns
  `complete: false` with the offending graphs listed. A caller may treat an unbound
  `OPTIONAL` as "no match" only for graphs that are `present` in a `complete`
  answer.
- Remote `SERVICE` rows whose endpoint reports no coordinate make the answer
  `complete: false`.
- What the answer does **not** do is decide, per unbound variable, which graph
  "should" have bound it. A caller who needs that must name the target graph and
  check its entry in the `cut`.

F2's acceptance includes the control that distinguishes the two cases: the same
`OPTIONAL` query with the target graph present but lacking the fact, and with the
target graph unavailable. Both return the variable unbound. Only the first may
carry `complete: true`.

### As of a time

Transaction time is per store; there is no shared transaction order across stores
and this design does not invent one. Therefore:

- `as_of_tx` is accepted only for a single store. A scalar `as_of_tx` over more
  than one store is refused, as the composition and per-graph designs already do.
- A cross-store historical read takes a **per-graph coordinate vector**
  (`{graph: (store_id, log_epoch, tx)}`), each validated against that store's log.
- **Valid time** is portable because it is data; `as_of_valid` may span stores.
  A mixed request (valid time across stores, transaction time per store) is
  allowed only in the vector form.

## 4. Trust

Graphs carry trust labels declared by the local operator, never read from a
remote ([multi-DB composition](multi-db-composition.md)). Peer graphs land in a
quarantine plane below inferred knowledge until they are promoted. Links follow
these rules:

- **Row trust is the minimum** over every graph that contributed a binding to the
  row, including the alignment graph when aliases were followed.
- **A link does not launder.** A trusted graph linking to an entity in a peer graph
  says only "the trusted owner points here". It confers no trust on the peer
  graph's facts about that entity.
- **Links out of quarantine are inert for inference.** Reasoning and derived
  facts do not consume edges whose asserting graph is below the configured trust
  floor, regardless of where the edge points.
- **Promotion is per graph, through the existing share-promotion path.** Promoting
  a peer graph promotes its outgoing links with it. It never promotes the graphs
  those links point into.
- **Corroboration** (the same edge asserted independently by several peers) is
  evidence recorded against the edge, not an automatic promotion.

## 5. Shapes

SHACL validation of a write today sees the payload, ROOT type triples and the
destination graph, and deliberately not a third graph; the library can pin up to
16 named context graphs. It does not see attached files or remote stores. Cross-store
links therefore need an explicit rule:

1. **Local constraints validate locally.** Cardinality, datatype and value
   constraints on `G`'s own triples are validated in `G` as today.
2. **A constraint that needs the neighbour declares its context.** For example,
   "the object of `<references>` must be a `WorkItem`" depends on `H`. The shape
   (or the graph's validation policy) declares `H` as context. The declared set is
   bounded; an undeclared or unbounded dependency is refused at shape load, not at
   write time.
3. **Context is pinned.** The validator reads declared context at a coordinate it
   records with the validation report: the same-host common cut, or a pinned as-of
   copy for a remote graph. It never reads a remote endpoint during a write.
4. **Missing context is `UNKNOWN`, not pass.** If declared context is unavailable,
   older than its freshness floor or from another lineage, the dependent
   constraint reports `UNKNOWN`. The graph's policy decides whether `UNKNOWN`
   refuses the write (strict) or records it as advisory (the local-sync rule for
   slices).
5. **Validation never repairs across stores.** A repair pass may only remove
   violations in the graph being written, as it does today.

## Build order

Smallest first. Each step is independently useful and lines up with the
[per-graph ingest pilot](per-graph-ingest-pilot.md), whose producers (code
projection, commit links, document mentions) are the first real cross-graph links:
they point from routed ingest graphs at entities that stay in ROOT.

| Step | Scope | Lines up with | Done when |
|---|---|---|---|
| F0 | This document; no code | Pilot step 1 (producer/consumer matrix) | Accepted |
| F1 | Read-only link audit for a graph (see "Link audit visibility" below) | Pilot step 1 and step 4 (composed reads) | Reports for the three pilot producers, with dangling links listed; a non-admin caller's report is identical whether or not the target exists in a graph it cannot read |
| F2 | `cut` vector and the answer-level `complete` flag in composed-read responses; refusal of scalar `as_of_tx` across files stays as is | Pilot step 4 | Negative controls: unavailable selected graph refuses (strict) or yields `complete: false` (vector); an `OPTIONAL` against an absent fact and against an unavailable graph are told apart only by `complete` |
| F3 | Opt-in alias mode over a decided alignment graph | After the pilot; independent of routing | Alias-justified rows stamped; revoked row stops matching; ungranted alignment graph refuses |
| F4 | Declared, pinned SHACL context for cross-store constraints, with `UNKNOWN` on missing context | Before any routed graph has a shape that depends on ROOT | Shape load refuses undeclared dependency; missing context yields `UNKNOWN`, never pass |
| F5 | Bounded `VALUES` bind join for `SERVICE` under the per-endpoint `disclose_from` rule | Independent; first remote consumer | Remote receives only the bound values; over-limit query refuses with the limit named; a binding from a graph outside `disclose_from` refuses and the capturing endpoint receives nothing |
| F6 | Trust minimum per row and the inference floor for quarantined graphs | Peer network work | A peer-graph edge never appears in derived facts; row trust equals the minimum contributor |

F1 and F2 need no new storage and no new write path, so they can run inside the
pilot's qualification without adding a second write-path PR.

### Link audit visibility

The F1 audit must not become an existence oracle for graphs the caller cannot
read. For an ordinary reader it reports exactly two buckets, computed only over
graphs in that reader's grant:

- **resolved**: the object IRI has facts in another granted graph;
- **unresolved**: it has facts in no granted graph.

An object that exists only in an ungranted graph is reported as unresolved, so the
report is identical whether or not it exists there. A third bucket, "present only
in a graph outside the grant", is available only to an operator with an
administrative read of the catalog, and is never returned on the reader routes.
F1's acceptance includes the negative control: the same audit run by a reader
with and without a hidden target graph produces byte-identical output.

## Open questions

1. Should the catalog's derived "links into" set (section 1) be persisted per
   generation or computed on demand by the F1 audit? Recommendation: on demand
   until a consumer needs it on the hot path.
2. What is the default bind-join limit `N` for F5? Recommendation: the existing
   `VALUES` size limit for local queries, so one number governs both.
3. Does alias mode (F3) apply to `SERVICE` results? Recommendation: no, not in F3;
   remote rows join on exact IRIs only until there is a remote alignment contract.
4. Who decides whether a graph's validation policy treats `UNKNOWN` as a refusal?
   Recommendation: the graph owner, recorded in the graph's policy, with strict as
   the default for graphs that already validate in ROOT today.
