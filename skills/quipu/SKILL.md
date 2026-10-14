---
name: quipu
description: >-
  Query the Quipu knowledge graph before you act — what the aegis homelab already knows about a
  service, container, bead, or crew member, and what the graph is NOT to be trusted about.
  Portable: shell + HTTP to ${GRAPH_URL} only, runs from any LLM agent, no Gas Town formulas.
  Triggers on "query before you act", "what do we know about <thing>", "ask quipu", "check the
  ontology", "blast radius", "what depends on X", "what runs on X", or before starting any bead
  that names an entity. To ADD knowledge use graph-extract; to map the whole graph use
  graph-report. This skill is the read path.
allowed-tools:
  - Bash
  - Read
---

# quipu — query before you act

The aegis homelab's operational memory is an RDF graph at `${GRAPH_URL}`. It knows which
services depend on what, which host runs which container, what broke before and why. **Before you
start a bead, ask the graph about the entities the bead names.** That is the whole skill.

This is the **read** path. Its two siblings:

- **[graph-extract]** — *write*. Add knowledge (`POST /episode`). Do not hand-roll ingestion; use it.
- **[graph-report]** — *orient*. Map the whole graph (god nodes, communities, surprises).

## Essential Principles

1. **A PREFIX declaration is REQUIRED.** Bare `rdfs:label` or `a:depends_on` is a hard parse error
   (`SPARQL parse error: ... Prefix not found`) — the endpoint declares no prefixes for you. This is
   the single most common way a first query fails. Copy the prefix block below every time.

2. **Keep the SPARQL on ONE line, or encode it with `jq`.** A JSON string cannot contain literal
   newlines, so the natural pretty-printed `-d '{"query":"\n  PREFIX ..."}'` fails with
   `control character (\x00-\x1f) found while parsing a string` — a JSON error that says nothing
   about SPARQL, so it reads like the graph is broken when it's just the quoting. SPARQL ignores
   whitespace; one long line is fine. For long queries use the heredoc + `jq` form in step 2.

3. **The response shape is content-negotiated — pick the one that fits (internal-ref, live).** The
   `/query` endpoint honours the `Accept:` header:
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
   Note: even the standard JSON path still **does not** carry datatype/language tags faithfully
   (internal-ref) — so for datatyped/tagged literals, treat values as strings, not typed RDF terms.

4. **One real thing is many nodes — never trust a clean-looking answer.** Entity resolution has
   never run (see Limits). "Dolt" is *seven* separate `DatabaseService` nodes today. A blast-radius
   query returns a **subset that looks exactly like a complete answer**. This is worse than an error,
   because an error tells you it failed. Always widen with a `regex` label scan first.

5. **Ask the graph to show its work.** Counts drift hourly — this graph gains facts all day. Ship
   *queries*, not remembered numbers. If you need a number, re-run the query; do not quote one from
   a doc or a bead (including this file). *(Measured: every predicate count in this skill's own
   source bead moved within 12 hours — `depends_on` 30→33, `prov:wasGeneratedBy` 950→991.)*

## Workflow

### 1. Cheapest first — semantic search

You rarely know the exact IRI. Start here; it is one call and needs no SPARQL:

```bash
curl -s ${GRAPH_URL}/search -X POST -H 'Content-Type: application/json' \
  -d '{"query":"dolt database server"}'
```

Returns scored hits with a `text` summary and the real `entity` IRI:
`{"count":10,"results":[{"entity":"http://aegis.gastown.local/ontology/dolt-server.service",
"score":0.647,"text":"dolt-server.service. Dolt SQL server on ${DB_HOST}:3306, the data plane for
beads. ... Fragile; reactor previously died for 9 weeks via systemd stop-propagation from this
unit.","valid_from":"..."}]}`

That `text` is often the entire answer. Take the `entity` IRI into step 2.

### 2. SPARQL — two forms that work

**Short query — one line** (copy-paste, no dependencies). Note the escaped `\"` inside the JSON:

```bash
curl -s ${GRAPH_URL}/query -X POST -H 'Content-Type: application/json' \
  -d '{"query":"PREFIX a: <http://aegis.gastown.local/ontology/> PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#> SELECT ?s ?l WHERE { ?s rdfs:label ?l . FILTER(regex(?l, \"dolt\", \"i\")) } LIMIT 20"}'
```

**Long query — heredoc + `jq`** (readable, handles quotes/newlines correctly). Prefer this once a
query outgrows one line:

```bash
read -r -d '' Q <<'SPARQL'
PREFIX a:    <http://aegis.gastown.local/ontology/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
PREFIX rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
PREFIX prov: <http://www.w3.org/ns/prov#>
SELECT ?s ?l WHERE { ?s rdfs:label ?l . FILTER(regex(?l, "dolt", "i")) } LIMIT 20
SPARQL
curl -s ${GRAPH_URL}/query -X POST -H 'Content-Type: application/json' \
  -d "$(jq -n --arg q "$Q" '{query:$q}')"
```

The heredoc is quoted (`<<'SPARQL'`) so the shell leaves `?s` and `$` alone, and `jq` does the JSON
escaping — so you write plain `"dolt"` instead of `\"dolt\"`. Both forms verified live.

`a:` is the aegis base — **classes and instances share it** (`a:LXCContainer` the class,
`a:${DB_HOST}` the instance).

### 3. Find every node for your entity — do this before any traversal

```sparql
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?s ?l WHERE { ?s rdfs:label ?l . FILTER(regex(?l, "<thing>", "i")) } LIMIT 20
```

If this returns several nodes for one real-world thing (it usually does), **every** subsequent
query must account for all of them. Verified live: a `regex "dolt"` label scan returns `a:dolt`,
`a:Dolt`, `a:${DB_HOST}`, `a:dolt-server`, `a:dolt-server.service`, `a:dolt-aegis-db`,
`a:local-dolt-fork` — seven nodes, one Dolt.

### 4. What is this thing / what do we know about it

```sparql
PREFIX a: <http://aegis.gastown.local/ontology/>
SELECT ?p ?o WHERE { a:dolt-server.service ?p ?o }
```

`rdfs:comment` carries the prose an earlier agent wrote. `prov:wasGeneratedBy` points at the
*episode* that asserted the fact — follow it to learn *when and why* something was recorded.

### 5. Traversal — property paths DO work

Verified live: `depends_on+` (transitive) returns strictly more than plain `depends_on`, and
alternation `depends_on|runs_on` returns exactly the sum of both. So blast-radius queries are
viable **today**:

```sparql
PREFIX a: <http://aegis.gastown.local/ontology/>
SELECT ?dependent WHERE { ?dependent a:depends_on+ a:${DB_HOST} }
```

Re-run the pair below yourself to confirm paths still work before relying on one:
```sparql
SELECT (COUNT(*) AS ?n) WHERE { ?s a:depends_on  ?o }   # plain
SELECT (COUNT(*) AS ?n) WHERE { ?s a:depends_on+ ?o }   # transitive — must be >= plain
```

**But read Limit (a) before you believe the result.** The query above returns `reactor` for
`a:${DB_HOST}` — and `0` for `a:dolt`. Same real-world thing, different node, opposite answer.

### 6. What predicates exist (the graph's actual vocabulary)

Do not guess relationship names — ask:

```sparql
SELECT ?p (COUNT(*) AS ?n) WHERE { ?s ?p ?o } GROUP BY ?p ORDER BY DESC(?n) LIMIT 20
```

Today's domain predicates, in rough frequency order (the `prov:`/`rdfs:`/`rdf:` ones above them are
structural): `groupId`, `prov:wasAssociatedWith`, `invocation`, `capability_kind`, `runs_on`,
`depends_on`, `contentHash`, `authored_by`, `owns`, `applies_to`. Full vocabulary and entity types:
`{baseDir}/../graph-extract/references/taxonomy.md`.

### 7. Graph size / liveness

```bash
curl -s ${GRAPH_URL}/stats     # {"entities":N,"facts":N,"predicates":N}
curl -s ${GRAPH_URL}/report    # nodes, edges, communities, hubs, suggested_questions
```

## Limits — say these out loud before you trust an answer

These are **live-verified**, not folklore. Each is a real bead. They are the difference between
using the graph and being misled by it.

**(a) Entity resolution has NEVER fired** (`resolution.enabled` defaults false and the deployed
config never sets it — **internal-ref**). One real thing is many nodes. Verified live: seven separate
nodes typed `DatabaseService` match `regex "dolt"`. Consequence: `?x depends_on+ a:dolt` returns
**0** while `?x depends_on+ a:${DB_HOST}` returns **reactor**. A blast-radius answer is a *subset
presented as a whole* — for triage that is worse than no answer. **Mitigation:** always run the
step-3 label scan and union across every node you find.

**(b) SHACL validates NOTHING** (0 shapes loaded; `validate_on_write` defaults false —
**internal-ref**). Verified live: `POST /shapes` → `{"count":0,"shapes":[]}`. Nothing constrains what
gets written; a fact in the graph passed no schema check. `POST /validate` requires you to supply
`shapes` inline — there is no stored shape set.

**(c) `owl:sameAs` and `quipu:distinctFrom` have 0 uses.** Verified live. So there is not even a
manual alias layer papering over (a) — you cannot look up "what else is this same thing".

**(d) These limits are the internal-ref pattern.** A capability is present, its flag defaults off, the
deployed config never sets it, and every surface signal reports healthy. Endpoint answers ≠ endpoint
acts. Ask any capability to show its work — `count > 0` — before believing it.

**(e) Asking "what is MISSING?" — the engine refuses the obvious idioms, and a careless client turns
the refusal into a clean answer.** Verified live 2026-07-15. The SPARQL engine does **not** support:

```
FILTER NOT EXISTS { ... }     FILTER(?x IN (...))     VALUES ?x { ... }     MINUS { ... }
```

Each returns **HTTP 400** with `{"error": "unsupported ..."}` and **no `"rows"` key**. The server is
honest — *your client is where the lie gets made*:

```python
rows = d.get("rows", [])        # ← a 400 refusal becomes []  →  "0 results, graph is clean"
```

**Check the status code** (it is 400 — that alone catches all four), or check for an `error` key
**before** reading `rows`. An errored query is not a *zero* result, it is *no* result.

Use the sanctioned negation idiom instead — verified exact, `565 + 34 = 599 = /stats` entities:

```sparql
SELECT ?s WHERE { ?s ?p ?o . OPTIONAL { ?s aegis:runs_on ?x } FILTER(!bound(?x)) }
```

> **A gap-detector that has never returned a row is indistinguishable from a broken one.** Run a
> positive control on a predicate you can *prove* most nodes lack, and confirm it returns rows,
> before you believe a zero. (maldoon's rule — he escaped a fabricated "0 ghosts — graph is clean"
> only by forcing that control; ellie reached the same standard independently from the close side.
> Trap found by ian on internal-ref.)

## When to use

- **Before starting any bead** — search the entities it names (step 1). Two minutes here beats
  rediscovering that `dolt-server.service` propagates systemd stops.
- **Before touching a service** — blast radius (step 5), then widen per Limit (a).
- **When a bead references history** ("this broke before") — the `rdfs:comment` + episode trail.

## How this reaches you (and when it doesn't)

You don't have to remember this skill exists. A **SessionStart hook** (matcher `startup`) injects a
short live orientation — current entity/fact counts, the search + SPARQL recipes, and the trust
limit — into every launcher-started crew session. It fetches `/stats` live with a 3s timeout and
**fails silent** (exit 0, no output) if Quipu is unreachable, so it can never wedge a session.

Registered in two places, deliberately:
- `~/.gt/hooks-overrides/${BEADS_DB}__crew.json` — the **source of truth**. `gt hooks` generates
  each `settings.json` from base+overrides, so a hand-edit alone would be erased by the next
  `gt hooks sync` (**internal-ref**). The override is what makes the hook survive that.
- `${BEADS_DB}/crew/.claude/settings.json` — the **live** copy, so it fires today. `gt hooks sync`
  is currently unsafe to run for unrelated reasons (it would strip Bobbin hooks and the x9hw Stop
  hook — see internal-ref), so the generated file had to be updated directly. `gt hooks diff`
  confirms sync would reproduce this entry identically, so it is not drift.

Matcher `startup` is used rather than `""` on purpose: base already owns `SessionStart ""`
(`gt prime --hook`), and a same-matcher override **replaces** the base entry — which would have
silently deleted priming.

**KNOWN GAP — it does NOT fire in handoff-respawned sessions.** `gt handoff` drops `--settings` on
respawn, so a cycled session loads *no* crew hooks at all (**internal-ref**, arnold). If you got here
via a handoff, you received no orientation — that is expected until 05up lands, and it is why this
file must stand on its own.

**Keep hook output under ~2KB.** SessionStart stdout above roughly that is truncated to a preview
and spilled to a file the model won't read. (`gt prime --hook` emits ~19.7KB, so most of the crew
primer never reaches the agent.) This block is ~1.4KB by design.

## Adding knowledge

Use **[graph-extract]** (`POST /episode`, group `${GRAPH_GROUP}`). Do not hand-roll a POST — it
handles node/edge shape, provenance, and the taxonomy. Per the aegis execution model, knowledge must
reach Quipu, not just a document.

[graph-extract]: ../graph-extract/SKILL.md
[graph-report]: ../graph-report/SKILL.md
