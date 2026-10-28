---
name: quipu
description: >-
  Query the Quipu knowledge graph before you act — what your graph already knows about a
  service, container, item, or agent, and what the graph is NOT to be trusted about.
  Portable: shell + HTTP to ${GRAPH_URL} only, runs from any LLM agent, no framework.
  Triggers on "query before you act", "what do we know about an entity", "ask quipu", "check the
  ontology", "blast radius", "what depends on X", "what runs on X", or before starting any item
  that names an entity. To ADD knowledge use graph-extract; to map the whole graph use
  graph-report. This skill is the read path.
allowed-tools:
  - Bash
  - Read
---

# quipu — query before you act

Your operational memory is an RDF graph at `${GRAPH_URL}`. It knows which services depend on
what, which host runs which container, what broke before and why. **Before you start a work item,
ask the graph about the entities the item names.** That is the whole skill.

This is the **read** path. Its two siblings:

- **[graph-extract]** — *write*. Add knowledge (`POST /episode`). Do not hand-roll ingestion; use it.
- **[graph-report]** — *orient*. Map the whole graph (god nodes, communities, surprises).

The examples use the base IRI `http://aegis.gastown.local/ontology/` (prefix `a:`), the example
namespace used throughout Quipu's own docs and tests. **Replace it with your graph's base IRI.**

## Search confidence before using context

Ranked `/search` candidates are not proof that an answer exists. With the
`quipu-context/v1` response contract, render **NO CONTEXT** and the bounded reason
when `insufficient_context` is true; do not quote or inject the diagnostic
`results`. When it is false, use only `context_results`, retaining each hit's
source and `valid_from`. An admitted historical fact is not current state or a
verified cause. A missing/unknown contract, malformed response or failed request
is **UNKNOWN**, not zero hits and not permission to use the nearest candidate.

The provisional 0.52 raw-cosine floor has a narrow measured margin. It must pass
alongside exact subject/identifier and term coverage; neither rank nor a fused
score alone admits context. See the REST API search contract for the explicit
reasons and limits. Do not lower a floor or retry queries merely to obtain a hit.

## Essential Principles

1. **A PREFIX declaration is REQUIRED.** Bare `rdfs:label` or `a:depends_on` is a hard parse error
   (`SPARQL parse error: ... Prefix not found`) — the endpoint declares no prefixes for you. This is
   the single most common way a first query fails. Copy the prefix block below every time.

2. **Keep the SPARQL on ONE line, or encode it with `jq`.** A JSON string cannot contain literal
   newlines, so the natural pretty-printed `-d '{"query":"\n  PREFIX ..."}'` fails with
   `control character (\x00-\x1f) found while parsing a string` — a JSON error that says nothing
   about SPARQL, so it reads like the graph is broken when it's just the quoting. SPARQL ignores
   whitespace; one long line is fine. For long queries use the heredoc + `jq` form in step 2.

3. **The response shape is content-negotiated — pick the one that fits.** The `/query` endpoint
   honours the `Accept:` header:
   - **Default / no `Accept` / `application/json`** — Quipu's own compact shape:
     `{"count":N,"rows":[{"var":"value"}],"variables":[...],"truncated":bool}` — flat rows, values
     already unwrapped. `ASK` returns `{"result":true}`; `CONSTRUCT` returns
     `{"count":N,"triples":[{subject,predicate,object}]}`. Convenient, but **lossy**: a literal that
     looks like a URL is byte-identical to a real IRI, and datatypes/language tags are dropped — so
     don't infer "this is a node" from a value that starts with `http`.
   - **`Accept: application/sparql-results+json`** — standard **W3C SPARQL 1.1 Results JSON**:
     `{"head":{"vars":[...]},"results":{"bindings":[{"var":{"type":"uri"|"literal","value":...}}]}}`.
     Use this when you need to tell an IRI from a literal — the `type` field disambiguates what the
     default shape flattens away. `results.bindings[].value` **does** exist on this path.
   - **`Accept: text/turtle` / `application/n-triples`** (CONSTRUCT only) — RDF serialization.
   Note: even the standard JSON path may not carry datatype/language tags faithfully, so for
   datatyped or tagged literals treat values as strings, not typed RDF terms.

4. **One real thing can be many nodes — never trust a clean-looking answer.** Unless your
   deployment resolves entities, the same database can exist as several nodes with different
   spellings. A blast-radius query then returns a **subset that looks exactly like a complete
   answer**. This is worse than an error, because an error tells you it failed. Always widen with
   a `regex` label scan first.

5. **Ask the graph to show its work.** Counts drift — a live graph gains facts all day. Ship
   *queries*, not remembered numbers. If you need a number, re-run the query; do not quote one from
   a doc or a work item (including this file).

## Workflow

### 1. Cheapest first — semantic search

You rarely know the exact IRI. Start here; it is one call and needs no SPARQL:

```bash
curl -s ${GRAPH_URL}/search -X POST -H 'Content-Type: application/json' \
  -d '{"query":"postgres database server"}'
```

Returns scored hits with a `text` summary and the real `entity` IRI, shaped like:
`{"count":10,"results":[{"entity":"http://aegis.gastown.local/ontology/postgres.service",
"score":0.647,"text":"postgres.service. Primary database for the billing API. ...",
"valid_from":"..."}]}`

That `text` is often the entire answer. Take the `entity` IRI into step 2.

### 2. SPARQL — two forms that work

**Short query — one line** (copy-paste, no dependencies). Note the escaped `\"` inside the JSON:

```bash
curl -s ${GRAPH_URL}/query -X POST -H 'Content-Type: application/json' \
  -d '{"query":"PREFIX a: <http://aegis.gastown.local/ontology/> PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#> SELECT ?s ?l WHERE { ?s rdfs:label ?l . FILTER(regex(?l, \"postgres\", \"i\")) } LIMIT 20"}'
```

**Long query — heredoc + `jq`** (readable, handles quotes/newlines correctly). Prefer this once a
query outgrows one line:

```bash
read -r -d '' Q <<'SPARQL'
PREFIX a:    <http://aegis.gastown.local/ontology/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
PREFIX rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
PREFIX prov: <http://www.w3.org/ns/prov#>
SELECT ?s ?l WHERE { ?s rdfs:label ?l . FILTER(regex(?l, "postgres", "i")) } LIMIT 20
SPARQL
curl -s ${GRAPH_URL}/query -X POST -H 'Content-Type: application/json' \
  -d "$(jq -n --arg q "$Q" '{query:$q}')"
```

The heredoc is quoted (`<<'SPARQL'`) so the shell leaves `?s` and `$` alone, and `jq` does the JSON
escaping — so you write plain `"postgres"` instead of `\"postgres\"`.

In the example namespace **classes and instances share the base** (`a:LXCContainer` the class,
`a:db-host-1` the instance).

### 3. Find every node for your entity — do this before any traversal

```sparql
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?s ?l WHERE { ?s rdfs:label ?l . FILTER(regex(?l, "<thing>", "i")) } LIMIT 20
```

If this returns several nodes for one real-world thing (it often does), **every** subsequent
query must account for all of them. For example, a `regex "postgres"` label scan might return
`a:postgres`, `a:Postgres`, `a:postgres.service` and the host `a:db-host-1` — four nodes, one
database.

### 4. What is this thing / what do we know about it

```sparql
PREFIX a: <http://aegis.gastown.local/ontology/>
SELECT ?p ?o WHERE { a:postgres.service ?p ?o }
```

`rdfs:comment` carries the prose an earlier agent wrote. `prov:wasGeneratedBy` points at the
*episode* that asserted the fact — follow it to learn *when and why* something was recorded.

### 5. Traversal — property paths work

`depends_on+` (transitive) should return at least as much as plain `depends_on`, and alternation
`depends_on|runs_on` returns the sum of both. So blast-radius queries are viable:

```sparql
PREFIX a: <http://aegis.gastown.local/ontology/>
SELECT ?dependent WHERE { ?dependent a:depends_on+ a:db-host-1 }
```

Re-run the pair below yourself to confirm paths work on your deployment before relying on one:

```sparql
SELECT (COUNT(*) AS ?n) WHERE { ?s a:depends_on  ?o }   # plain
SELECT (COUNT(*) AS ?n) WHERE { ?s a:depends_on+ ?o }   # transitive — must be >= plain
```

**But read Limit (a) before you believe the result.** If the edges were recorded against
`a:db-host-1`, the query above finds `billing-api` there and returns `0` for `a:postgres`. Same
real-world thing, different node, opposite answer.

### 6. What predicates exist (the graph's actual vocabulary)

Do not guess relationship names — ask:

```sparql
SELECT ?p (COUNT(*) AS ?n) WHERE { ?s ?p ?o } GROUP BY ?p ORDER BY DESC(?n) LIMIT 20
```

The `prov:`/`rdfs:`/`rdf:` predicates are structural; the rest are your domain vocabulary
(`runs_on`, `depends_on`, `owns`, ...). The graph-extract skill's taxonomy reference lists the
entity types and relationships it writes.

### 7. Graph size / liveness

```bash
curl -s ${GRAPH_URL}/stats     # {"entities":N,"facts":N,"predicates":N}
curl -s ${GRAPH_URL}/report    # nodes, edges, communities, hubs, suggested_questions
```

## Limits — say these out loud before you trust an answer

They are the difference between using the graph and being misled by it. Check each one against
your own deployment; the examples describe what one deployment found.

**(a) Entity resolution may not be running.** If it is off, one real thing is many nodes, as in
step 3. Consequence: `?x depends_on+ a:postgres` can return **0** while
`?x depends_on+ a:db-host-1` returns **billing-api**. A blast-radius answer is then a *subset
presented as a whole* — for triage that is worse than no answer. **Mitigation:** always run the
step-3 label scan and union across every node you find.

**(b) Do not assume a schema checked what you read.** Ask whether shapes are loaded and enforced
(`POST /shapes`) before treating a fact as validated. In one deployment the shape list came back
empty, so nothing constrained what had been written.

**(c) Do not assume an alias layer.** Check that `owl:sameAs` (or your deployment's distinctness
predicate) is actually populated before relying on it to answer "what else is this same thing".

**(d) The pattern behind (a)–(c):** a capability is present, its flag defaults off, the deployed
config never sets it, and every surface signal reports healthy. An endpoint that answers is not an
endpoint that acts. Ask any capability to show its work — `count > 0` — before believing it.

**(e) Asking "what is MISSING?" — the engine may refuse the obvious idioms, and a careless client
turns the refusal into a clean answer.** Observed on Quipu 0.13; re-check on your version. The
SPARQL engine did **not** support:

```text
FILTER NOT EXISTS { ... }     FILTER(?x IN (...))     VALUES ?x { ... }     MINUS { ... }
```

Each returns **HTTP 400** with `{"error": "unsupported ..."}` and **no `"rows"` key**. The server is
honest — *your client is where the lie gets made*:

```python
rows = d.get("rows", [])        # ← a 400 refusal becomes []  →  "0 results, graph is clean"
```

**Check the status code** (it is 400 — that alone catches all four), or check for an `error` key
**before** reading `rows`. An errored query is not a *zero* result, it is *no* result.

Use the supported negation idiom instead, and check it: the nodes it returns plus the nodes that do
have the predicate should add up to the entity count from `/stats`.

```sparql
PREFIX a: <http://aegis.gastown.local/ontology/>
SELECT ?s WHERE { ?s ?p ?o . OPTIONAL { ?s a:runs_on ?x } FILTER(!bound(?x)) }
```

> **A gap-detector that has never returned a row is indistinguishable from a broken one.** Run a
> positive control on a predicate you can *prove* most nodes lack, and confirm it returns rows,
> before you believe a zero.

## When to use

- **Before starting any work item** — search the entities it names (step 1). Two minutes here
  beats rediscovering a dependency the graph already knew.
- **Before touching a service** — blast radius (step 5), then widen per Limit (a).
- **When an item references history** ("this broke before") — the `rdfs:comment` + episode trail.

## Adding knowledge

Use **[graph-extract]** (`POST /episode`, group `${GRAPH_GROUP}`). Do not hand-roll a POST — it
handles node/edge shape, provenance, and the taxonomy. Knowledge that only reaches a document is
lost to the next agent; it has to reach the graph.

[graph-extract]: https://github.com/scbrown/camayoc/tree/main/skills/graph-extract
[graph-report]: https://github.com/scbrown/camayoc/tree/main/skills/graph-report

## Agent primer

The block below is `quipu prime --static` with headings shifted down one level. With the CLI
installed, `quipu prime` prints it plus a live summary of the local store.

<!-- BEGIN quipu prime: generated from src/primer.md; regenerate with QUIPU_BLESS_GOLDENS=1 cargo test --test prime_sync -->
### Quipu agent primer

#### What Quipu is

Quipu is a knowledge graph for agents: RDF facts in one SQLite file, queried with SPARQL.
Writes are checked against SHACL shapes, so the graph refuses data its vocabulary does not allow.
Every fact keeps its transaction and valid time, so you can ask what was true at any point.
ROOT is the default graph; named graphs hold scoped knowledge and are selected explicitly.
`quipu` (alias `qp`) works on a local store; `quipu mcp` and `quipu-server` serve the same tools to agents.

#### Query first

1. Before you act on an entity, ask the graph what it already knows: `quipu search`, then `quipu read`.
2. Prefer stored queries (`quipu_ask`) over hand-written SPARQL when one fits.
3. Declare every PREFIX; the endpoint declares none for you.
4. Name the dataset: ROOT unless you pass a graph (`FROM`, `FROM NAMED`, or the `graph` field).
5. Treat an empty or failed answer as unknown, not as proof of absence.
6. Write only facts you can source, through a validated path (`knot`, `episode`), never by editing the file.

#### Core commands

| Command | Use |
| --- | --- |
| `quipu prime` | Print this primer and a summary of the local store |
| `quipu read "<sparql>"` | Run a SPARQL query (`--valid-at`, `--tx` for time travel; alias `query`) |
| `quipu search "<text>"` | Keyword search for entities (`--mode` selects vector or hybrid) |
| `quipu cord --type <IRI>` | List entities, optionally of one type |
| `quipu impact <IRI>` | Walk what depends on an entity (`--remove` for a what-if) |
| `quipu unravel --valid-at <date>` | See the graph as it was at a point in time |
| `quipu explain <s> <p> <o>` | Trace a derived fact back to its premises |
| `quipu knot <file.ttl>` | Assert Turtle facts, validated against loaded shapes (alias `load`) |
| `quipu episode <file.json>` | Ingest a structured episode of nodes and edges |
| `quipu retract <IRI>` | Retract an entity's facts (history is kept) |
| `quipu validate --shapes <s> --data <d>` | Check data against shapes without writing |
| `quipu shapes list` | Show the SHACL shapes that guard writes |
| `quipu graph list` | List registered named graphs |
| `quipu stats` / `quipu doctor labels` | Store statistics and a graph-label drift report |
| `quipu export` | Export RDF from ROOT or one graph |
| `quipu share` / `quipu import` | Hand a graph to another store and verify one in |
| `quipu mcp` | Serve the MCP tools over stdio |

Run `quipu help <command>` for the exact syntax of any command.

#### MCP tools

| Tool | Use |
| --- | --- |
| `quipu_search` | Semantic, keyword or hybrid search for entities |
| `quipu_context` | Entities and facts around a natural-language question |
| `quipu_query` | SPARQL SELECT, with `valid_at` / `tx` time travel |
| `quipu_ask` | Run a stored, parameterized query by name (no name lists them) |
| `quipu_cord` | List entities by type or predicate |
| `quipu_impact` | Downstream walk from an entity, optionally counterfactual |
| `quipu_unravel` | Facts as they were at a point in time |
| `quipu_resolve_entity` | Check for near-duplicate entities before writing |
| `quipu_shapes` | Inspect the loaded shapes and class vocabulary |
| `quipu_validate` | Validate RDF against shapes without writing |
| `quipu_knot` | Assert Turtle facts with SHACL validation |
| `quipu_episode` | Ingest an episode of nodes and edges |
| `quipu_retract` | Retract facts for an entity |
| `quipu_graph_list` | List named graphs |
| `quipu_stats` / `quipu_health` | Serving store statistics and liveness |

#### Writing safely

- Check `quipu_resolve_entity` (or search) first, so you extend an entity instead of duplicating it.
- Use types and predicates the loaded shapes define; a refusal names the violated shape.
- Give writes a timestamp and a source you can point back to.
- Schema changes go through proposals (`quipu propose submit`, `quipu_propose_schema_change`), not direct shape edits.

#### Next steps

- Wire Quipu into your agent: <https://intigraph.github.io/quipu/getting-started/agent-setup.html>
- Full CLI reference: <https://intigraph.github.io/quipu/reference/cli.html>
- MCP tool reference: <https://intigraph.github.io/quipu/reference/mcp-tools.html>
<!-- END quipu prime -->
