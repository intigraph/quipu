# Coming from TerminusDB

`scripts/terminusdb-import.py` moves a TerminusDB database into Quipu in one
command. Your schema becomes SHACL shapes and your documents become one
episode. You can also replay the commit log as Quipu's transaction history.
Why move? Quipu adds valid time alongside commit history, value constraints
that SHACL can state, and SPARQL 1.1.

The script uses only the Python standard library. It needs no TerminusDB
client, only the open endpoints of the community build:

| Endpoint | Used for |
|---|---|
| `GET /api/log/<org>/<db>/local/branch/<branch>` | the head commit, and the history for `--replay-log` |
| `GET /api/triples/<org>/<db>/local/commit/<id>/schema` | the schema graph, as Turtle in TerminusDB's `sys:` vocabulary |
| `GET /api/triples/<org>/<db>/local/commit/<id>/instance` | your documents, as Turtle |

Both graphs are read from the **same commit**, so the shapes and the data
always match, even if someone commits while the import runs.

## One command

```bash
export TERMINUSDB_USER=admin TERMINUSDB_PASSWORD=...   # or TERMINUSDB_TOKEN
python3 scripts/terminusdb-import.py \
  --server http://localhost:6363 --db admin/mydb \
  --out ./from-terminusdb \
  --quipu-url http://localhost:3030
```

This writes `shapes.ttl`, `instance.nt` and `import.json` (a report) to
`--out`. It then loads the shapes with `POST /shapes` and the documents with
one `POST /knot`. That transaction's `source` names the origin commit:

```text
terminusdb:admin/mydb/local/commit/<commit-id>
```

`valid_from` is the commit time. To undo the whole import later, retract by
that source (`POST /retract/source`).

To load into a local store file instead of a server, pass `--quipu-db <path>`
and `--graph <iri>`. The script then runs `quipu shapes load` and
`quipu ingest --source <commit source>`. To convert without loading anything,
leave out both `--quipu-url` and `--quipu-db`.

You can also work from Turtle files you exported earlier:

```bash
python3 scripts/terminusdb-import.py \
  --schema-ttl schema.ttl --instance-ttl instance.ttl \
  --db admin/mydb --commit <id> --commit-time 1714400000 --out ./out
```

## How the schema maps

| TerminusDB | Quipu (SHACL) |
|---|---|
| `Class` | `sh:NodeShape` at `<Class>/Shape`, with `sh:targetClass <Class>` |
| plain property `"name": "xsd:string"` (mandatory) | `sh:minCount 1 ; sh:maxCount 1` |
| `Optional` | `sh:maxCount 1` |
| `Set` | no count bounds |
| `Set`/`Cardinality` with `@min_cardinality`, `@max_cardinality`, `@cardinality` | `sh:minCount` / `sh:maxCount` |
| an `xsd:` or `xdd:` range | `sh:datatype` |
| a class range | `sh:class`; for a class with subclasses, `sh:or` over the class and every subclass |
| `sys:JSON` | `sh:class sys:JSON` (JSON values are linked nodes, not literals) |
| `sys:Unit` | `sh:in ( rdf:nil )` (how TerminusDB stores a unit value) |
| an `Enum` range | `sh:in ( <Enum>/<value> ... )` |
| `@inherits` | `rdfs:subClassOf`, plus the parent's properties copied onto the child shape |
| `TaggedUnion` | `sh:xone` over its properties (exactly one present) |
| `@key` | an `rdfs:comment` on the shape naming the key fields (see below) |

TerminusDB gives each document only its most specific `rdf:type`, and Quipu
does not infer `rdfs:subClassOf` when it checks `sh:class`. So inherited
properties are copied onto the child shape, and a property whose range is a
parent class (`lead: Entity`) accepts any of the parent's subclasses through
`sh:or`.

TerminusDB's Turtle writer emits numbers in shorthand: a stored
`36^^xsd:decimal` comes out as `36`, which any Turtle reader types as
`xsd:integer`. The importer gives such numbers back the numeric datatype that
the schema declares for the property (or for its List or Array elements).
`numbers_retyped` in `import.json` counts how many it changed.

A few things do not translate fully. Each one is listed in
`import.json` under `not_translated` and printed as a `note:`:

- `List` and `Array` properties get a count bound, but no check on their
  elements' type. TerminusDB stores their values as `rdf:List` cons cells and
  `sys:Array` cells, not as the elements themselves. Shapes for both cell
  types are always included, because Quipu refuses an `rdf:type` that no
  loaded shape sanctions.
- `sys:JSON` values are checked as `sys:JSON` nodes. The JSON inside them is
  not validated.
- `@oneOf` is not translated.
- `Foreign` is kept as a plain IRI reference.
- A range the importer does not recognize gets only its count bounds, so it
  never refuses data.

Once the shapes are loaded, they gate every later write. The tests show these
violations refused: a missing mandatory field, too many values, a wrong
datatype, a value outside an enum and a reference to the wrong class. Anything
in the list above is checked less strictly than TerminusDB checks it, so
Quipu will accept some writes that TerminusDB would refuse. A SHACL refusal
comes back as HTTP 200 with `"conforms": false`. The script treats that as an
error and stops.

## IRIs and `@key`

By default every IRI is kept exactly as TerminusDB wrote it, so links from
other systems still resolve. Classes and properties live under
`terminusdb:///schema#` and documents under `terminusdb:///data/`. To move
them into your own namespaces, use `--rebase-schema https://example.org/ns#`
and `--rebase-data https://example.org/id/`.

For a `@key: Lexical` class, TerminusDB names each document after its key
values, and new records you write in Quipu should follow the same rule:

```text
<data base><Class>/<value 1>+<value 2>+...
```

Each value is URI-segment encoded, and a `+` inside a value becomes `%2B`. A
`Person` keyed on `name` with the name "Ada Lovelace" is
`terminusdb:///data/Person/Ada%20Lovelace`. `lexical_iri()` in the script
implements this rule. Documents with `Hash`, `ValueHash`, `Random` or `UuidV7`
keys keep the IRIs they already have.

## Replaying the commit log

```bash
python3 scripts/terminusdb-import.py --server http://localhost:6363 --db admin/mydb \
  --out ./replay --quipu-url http://localhost:3030 --replay-log [--log-count 100]
```

This walks `/api/log` oldest first. For each commit it reads that commit's
instance graph and posts it as a `replace_snapshot` knot. Facts that a commit
removed are retracted, and every write is stamped with:

- the commit time, as both the transaction time and `valid_from`;
- the commit, as the transaction's `source`.

Afterwards, `valid_at` and `tx` queries answer "what did the data say as of
commit N". Some details:

- Quipu keeps valid time to the second, but TerminusDB commit times are
  sub-second. A commit that lands in the same second as the one before it is
  moved to the next free second (`time_adjusted` in the report), so the order
  of commits is kept.
- Leading commits with an empty instance graph, such as the first,
  schema-only commit, are skipped.
- Every commit is checked against the head schema.
- Replay reads the full graph once per commit, so on a long history,
  `--log-count` limits it to the most recent N commits.

## Testing

`scripts/test-terminusdb-import.py` runs offline against two sets of
fixtures:

- `test-fixtures/terminusdb/live/`: real exports from a pinned
  `terminusdb/terminusdb-server:v12.0.8` container. These are the schema
  graph, the instance graph at every commit, and `/api/log`. The image digest
  and how to reproduce them are in that directory's README.
- `test-fixtures/terminusdb/`: fixtures built from TerminusDB's source
  (`json_schema_triple` in `src/core/document/json.pl`), plus an invalid
  instance graph.

When `quipu` and `quipu-server` are on `PATH`, the tests also validate every
real commit with the real engine, load over REST, and replay the real commit
log. CI runs them against the binaries it builds. To run the end-to-end test
against a TerminusDB server of your own, set `TERMINUSDB_LIVE_URL`,
`TERMINUSDB_USER` and `TERMINUSDB_PASSWORD`. In CI, the repository variable
`TERMINUSDB_LIVE_CI=true` starts the pinned container and runs that test.

See also the [TerminusDB gap analysis](https://github.com/intigraph/quipu/blob/main/docs/design/terminusdb-gap-analysis.md)
(section G6) and [SHACL Validation](../concepts/shacl-validation.md).
