# Quipu agent primer

## What Quipu is

Quipu is a knowledge graph for agents: RDF facts in one SQLite file, queried with SPARQL.
Writes are checked against SHACL shapes, so the graph refuses data its vocabulary does not allow.
Every fact keeps its transaction and valid time, so you can ask what was true at any point.
ROOT is the default graph; named graphs hold scoped knowledge and are selected explicitly.
`quipu` (alias `qp`) works on a local store; `quipu mcp` and `quipu-server` serve the same tools to agents.

## Query first

1. Before you act on an entity, ask the graph what it already knows: `quipu search`, then `quipu read`.
2. Prefer stored queries (`quipu_ask`) over hand-written SPARQL when one fits.
3. Declare every PREFIX; the endpoint declares none for you.
4. Name the dataset: ROOT unless you pass a graph (`FROM`, `FROM NAMED`, or the `graph` field).
5. Treat an empty or failed answer as unknown, not as proof of absence.
6. Write only facts you can source, through a validated path (`knot`, `episode`), never by editing the file.

## Core commands

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

## MCP tools

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

## Writing safely

- Check `quipu_resolve_entity` (or search) first, so you extend an entity instead of duplicating it.
- Use types and predicates the loaded shapes define; a refusal names the violated shape.
- Give writes a timestamp and a source you can point back to.
- Schema changes go through proposals (`quipu propose submit`), not direct shape edits.

## Next steps

- Wire Quipu into your agent: <https://intigraph.github.io/quipu/getting-started/agent-setup.html>
- Full CLI reference: <https://intigraph.github.io/quipu/reference/cli.html>
- MCP tool reference: <https://intigraph.github.io/quipu/reference/mcp-tools.html>
