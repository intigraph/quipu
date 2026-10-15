# Create and use a named graph through MCP

Use a local scratch store for this recipe. Install the CLI and sibling server,
then configure your MCP client with `quipu mcp --db ./tutorial.db`. Local MCP
opens no HTTP listener. A configured protected server needs its existing write
credential; an `actor` field declares provenance and never grants permission.
See [MCP connection setup](../reference/mcp-tools.md#connect-directly).

Each JSON block below is an MCP `tools/call` parameter object. Send the blocks
in order. Inspect `isError` before reading content; an error is not an empty
result. These exact blocks are exercised by the fresh-store acceptance test.
The timestamps are sample transaction times; use current UTC in your own work.

## 1. Create a committed graph

```json
{
  "name": "quipu_graph_create",
  "arguments": {
    "graph": "urn:example:graph:job-search",
    "actor": "urn:example:agent:researcher",
    "timestamp": "2026-10-14T12:00:00Z"
  }
}
```

Expect `created: true` and `creation` containing the original timestamp and declared actor. Repeating create returns `created: false` and preserves that provenance. An already interned IRI still needs registration.

## 2. Declare its labels

```json
{
  "name": "quipu_graph_label",
  "arguments": {
    "graph": "urn:example:graph:job-search",
    "timestamp": "2026-10-14T12:00:00Z",
    "actor": "urn:example:agent:researcher",
    "kind": "operational",
    "freshness": "fresh"
  }
}
```

## 3. Load the new class vocabulary

```json
{
  "name": "quipu_shapes",
  "arguments": {
    "action": "load",
    "name": "job-search",
    "timestamp": "2026-10-14T12:00:00Z",
    "turtle": "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix ex: <https://example.org/> . @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> . ex:JobShape a sh:NodeShape ; sh:targetClass ex:Job ; sh:property [ sh:path rdfs:label ; sh:minCount 1 ] ."
  }
}
```

This loads the vocabulary into the local store; supplying a type name on a write does not create or govern that class. Shape changes on a shared server need that server's normal schema authority.

## 4. Check the loaded vocabulary

```json
{
  "name": "quipu_shapes",
  "arguments": {
    "action": "vocabulary"
  }
}
```

Expect `https://example.org/Job` in `classes`. Use this exact class IRI in your RDF.

## 5. Write one job into the graph

```json
{
  "name": "quipu_knot",
  "arguments": {
    "graph": "urn:example:graph:job-search",
    "timestamp": "2026-10-14T12:00:00Z",
    "actor": "urn:example:agent:researcher",
    "turtle": "@prefix ex: <https://example.org/> . @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> . ex:job1 a ex:Job ; rdfs:label \"Research engineer\" ."
  }
}
```

## 6. Query this graph explicitly

```json
{
  "name": "quipu_query",
  "arguments": {
    "graph": "urn:example:graph:job-search",
    "query": "SELECT ?label WHERE { <https://example.org/job1> <http://www.w3.org/2000/01/rdf-schema#label> ?label }"
  }
}
```

Expect `count: 1` and label `Research engineer`. Omitting `graph` queries ROOT; it must return no matching job in this fresh store.

## 7. List registered graphs and their provenance

```json
{
  "name": "quipu_graph_list",
  "arguments": {}
}
```

Find the exact graph IRI, committed class, operational/fresh labels, and the same creation provenance from step 1. A ROOT row may also be present; compare the target row rather than assuming a total count.

## Fix common errors

- **Unknown graph / interned but not registered:** call `quipu_graph_create`
  with the exact graph IRI before `quipu_knot`. The error includes that call
  and the equivalent `quipu graph create '<iri>' --db <path>` command.
- **Unknown rdf:type / off-vocabulary class:** load a SHACL shape with
  `sh:targetClass` matching that exact class IRI using `quipu_shapes`, then
  check `action: vocabulary` before repeating the rejected write. On a shared
  store, propose the shape through its governed schema lane instead of changing
  somebody else's vocabulary without authority. A similarly spelled label or
  namespace is not a match.
- **SHACL validation failure:** supply the required label; do not disable
  validation. The recipe's Job shape requires `rdfs:label`.
- **Overlay graph:** use `quipu_overlay_write`; committed graph registration
  does not reclassify an overlay.

Creation provenance records caller-declared metadata. It is not proof that the
caller owned the actor IRI. Older or imported graphs may have an unknown actor;
repeating create never invents an original creator.
