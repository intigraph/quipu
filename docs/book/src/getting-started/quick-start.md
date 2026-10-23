# Quick Start

Install the [CLI and companion server](installation.md) first.

## A checked write and a refused write

Write a rule (every `Person` needs a name), two people who follow it, and one
who does not:

```bash
mkdir quipu-demo && cd quipu-demo
printf '@prefix sh: <http://www.w3.org/ns/shacl#> .\n@prefix ex: <http://example.org/> .\nex:PersonShape a sh:NodeShape ; sh:targetClass ex:Person ;\n  sh:property [ sh:path ex:name ; sh:minCount 1 ] .\n' > shapes.ttl
printf '@prefix ex: <http://example.org/> .\nex:alice a ex:Person ; ex:name "Alice" ; ex:knows ex:bob .\nex:bob   a ex:Person ; ex:name "Bob" .\n' > people.ttl
printf '@prefix ex: <http://example.org/> .\nex:carol a ex:Person .\n' > carol.ttl
```

Then load the good facts, ask a question, and try the bad one:

```bash
quipu knot people.ttl --shapes shapes.ttl --db demo.db
quipu read 'SELECT ?who ?name WHERE { ?who <http://example.org/name> ?name }' --db demo.db
quipu knot carol.ttl --shapes shapes.ttl --db demo.db
```

<!-- markdownlint-disable MD010 -->
<!-- quipu's real output is tab-separated; the block below is verbatim. -->

```text
SHACL validation passed
knotted 5 facts from people.ttl (tx 1)
who	name
----------------------------------------
http://example.org/alice	"Alice"
http://example.org/bob	"Bob"

2 results
SHACL validation failed: 1 violation(s)
  Violation on http://example.org/carol: MinCount(1) not satisfied
```

<!-- markdownlint-enable MD010 -->

The last command exits 1 and writes nothing: Carol breaks the rule, so she never
enters the graph. That refusal at write time, with the reason attached, is what
Quipu is for.

## On your own data

| you want to | run |
|---|---|
| load Turtle, checked against your shapes | `quipu knot <file.ttl> --shapes <shapes.ttl>` |
| bulk-load a lot of data at once, validated before it touches your graph | `quipu share --output <dir>` from a scratch store, then `quipu import <dir>` (staged and shape-checked; failures are quarantined), then `quipu import promote <share-id>` (one transaction into ROOT) |
| migrate or move a dataset between stores | the same three steps: `share`, `import`, `import promote` |
| ask a SPARQL question | `quipu read '<sparql>'` |
| see the graph as it was on a date | `quipu read '<sparql>' --valid-at 2026-09-01` |
| see what depends on an entity | `quipu impact <entity-IRI>` |
| keep shapes in the store, so every write is checked | `quipu shapes load <name> <shapes.ttl>` |
| explore it in a browser, or over HTTP | `quipu-server --db <file> --bind 127.0.0.1:3030` |

For a bulk load, a migration or a backfill, use **import + promote**, not
thousands of `knot` or SPARQL Update writes. `import` validates the whole dataset
in its own staging graph and never touches ROOT. `promote` then admits it in a
single transaction, and one fork rollback undoes it. Walkthrough:
[Bulk loads and migrations](https://github.com/scbrown/quipu/blob/main/docs/book/src/recipes/bulk-loads.md).

Every command and flag: [CLI reference](https://github.com/scbrown/quipu/blob/main/docs/book/src/reference/cli.md). The HTTP
endpoints: [REST API](https://github.com/scbrown/quipu/blob/main/docs/book/src/reference/rest-api.md).

New stores search all registered graphs minus configured `search.exclude_graphs`.
Existing stores retain ROOT until `search.default_graph_scope = "all"` is set;
SPARQL keeps its explicit dataset rules and ROOT default. Search responses report
graph coverage. Existing named-only semantic vectors need bounded preparation
before enabling them. See [named graphs, search, and promotion](https://github.com/scbrown/quipu/blob/main/docs/book/src/concepts/named-graphs.md#search-has-a-broader-default-than-sparql).

## Rust Library

```rust
use quipu::store::Store;
use quipu::rdf::ingest_rdf;
use quipu::sparql;
use oxrdfio::RdfFormat;

fn main() -> quipu::error::Result<()> {
    // Open a persistent store (or use open_in_memory() for testing)
    let mut store = Store::open("my-knowledge.db")?;

    // Ingest Turtle data
    let data = r#"
    @prefix ex: <http://example.org/> .
    @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

    ex:alice a ex:Person ;
        ex:name "Alice" ;
        ex:age "30"^^xsd:integer ;
        ex:knows ex:bob .

    ex:bob a ex:Person ;
        ex:name "Bob" ;
        ex:age "25"^^xsd:integer .
    "#;

    let (tx_id, count) = ingest_rdf(
        &mut store,
        data.as_bytes(),
        RdfFormat::Turtle,
        None,
        "2026-04-04T00:00:00Z",
        Some("demo"),
        Some("quick-start"),
    )?;
    println!("Ingested {count} triples in transaction {tx_id}");

    // Query with SPARQL
    let result = sparql::query(
        &store,
        r#"SELECT ?name ?age WHERE {
            ?s a <http://example.org/Person> .
            ?s <http://example.org/name> ?name .
            ?s <http://example.org/age> ?age .
            FILTER(?age >= 28)
        }"#,
    )?;

    println!("People aged 28+:");
    for row in result.rows() {
        println!("  {:?} age {:?}", row.get("name"), row.get("age"));
    }

    Ok(())
}
```

## CLI

```bash
# Load data
quipu knot data.ttl --db my.db

# Query
quipu read "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10" --db my.db

# Interactive REPL
quipu repl --db my.db
```

## REST API Server

```bash
# Start
quipu-server --db my.db --bind 0.0.0.0:3030

# Health check
curl localhost:3030/health

# Query
curl -s localhost:3030/query -X POST \
  -H "Content-Type: application/json" \
  -d '{"query": "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 5"}'

```

## Connect an agent

Use [Agent Setup](agent-setup.md) for stdio and HTTP MCP configuration,
authentication, verification and disconnecting a client.
