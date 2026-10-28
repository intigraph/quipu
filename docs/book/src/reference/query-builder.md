# Typed SPARQL query builders

Datalog where you want rules, SPARQL where you want queries, both over the same
bitemporal facts.

The Python REST client and JavaScript WASM helper compose immutable typed nodes
and serialize them as standard SPARQL 1.1 SELECT queries. Reusing a base query
does not change it. Variables, IRIs and literals have separate constructors;
ordinary strings never become raw query fragments.

## Python

```python
from quipu_client import (
    Aggregate, Compare, Filter, Group, Iri, Literal, Optional, Path,
    QuipuClient, Select, Triple, Union, Var,
)

s, label = Var("s"), Var("label")
base = Select((s, label)).where(Triple(s, Iri("http://example.org/label"), label))
q = base.where(Filter(Compare(label, "=", Literal("Snow: 雪 {literal}")))).limit(20)
print(q.to_sparql())

client = QuipuClient("http://localhost:3030", token="your-write-token")
rows = q.valid_at("2026-01-02T00:00:00Z").as_of_tx(42).execute(client)
q.save(client, "matching_labels", description="Labels matching this lexical value")
answer = client.ask("matching_labels")
```

`execute` sends the usual `/query` request. `save` sends an authenticated
`POST /queries` with `action: load`, the serialized template and `params: []`.
The server parses and validates the definition, versions replacements, and makes
it available through `/ask`. Auth, validation and storage refusals remain
`QuipuError`, including the response body; saving never retries automatically.

## JavaScript and WASM

Import `query-builder.mjs` beside the headless WASM loader, or from
`runtime/query-builder.mjs` in the browser release. The adjacent declaration
file describes the typed surface. `database` is the binding's real `Database`.

```javascript
import { Iri, Literal, Select, Triple, Var } from "./query-builder.mjs";

const s = new Var("s"), label = new Var("label");
const base = new Select([s, label]).where(
  new Triple(s, new Iri("http://example.org/label"), label));
const pinned = base.limit(20).validAt("2026-01-02T00:00:00Z").asOfTx(42);
const raw = pinned.execute(database);
base.save(database, "all_labels", { description: "Local label rows" });
const answer = database.invoke("quipu_ask", JSON.stringify({ name: "all_labels" }));
```

`execute` and `save` return the native binding's JSON text. This preserves native
64-bit result integers; parsing them into JavaScript Numbers is a separate,
explicit conversion decision. JavaScript transaction pins and limits must be
safe integers. Literal integers may also be supplied as `bigint`, which are
serialized directly into SPARQL text. A missing or older binding refuses an
unsupported operation; the helper never substitutes an alternate write path.

## Composition

Python uses tuples below; JavaScript uses arrays and the same node constructors.

| Node | Example | SPARQL |
| --- | --- | --- |
| Triple | `Triple(s, Iri("urn:p"), label)` | `?s <urn:p> ?label .` |
| Property path | `Path("/", (Iri("urn:p"), Path("+", (Iri("urn:q"),))))` | Sequence and repetition |
| Other paths | `Path("^", (Iri("urn:p"),))` | Inverse; also `\|`, `*`, `?` |
| Optional | `Optional(Group((triple,)))` | `OPTIONAL { ... }` |
| Union | `Union((Group((left,)), Group((right,))))` | `{ ... } UNION { ... }` |
| Filter | `Filter(Compare(label, "=", Literal("value")))` | Typed comparison |
| Function | `Call("STR", (label,))` | Whitelisted SPARQL built-in |
| Named graph | `Graph(Iri("urn:g"), Group((triple,)))` | `GRAPH <urn:g> { ... }` |

`Aggregate("COUNT", s, Var("total"))` projects a count. Use
`Select((kind, aggregate)).where(...).group_by(kind)` in Python, or
`new Select([kind, aggregate]).where(...).groupBy(kind)` in JavaScript.
Supported aggregates are COUNT, SUM, AVG, MIN, MAX and SAMPLE; COUNT accepts
`None`/`null` for `*`. Non-aggregate projected variables must appear in GROUP BY.

`Iri` takes an absolute IRI. `Literal` accepts strings, integers and booleans;
use a string plus a datatype IRI for decimal/date lexical forms, or a language
tag for localized text. Literal quotes, backslashes, controls and Unicode are
escaped without changing their value. Braces are escaped as Unicode sequences
so a literal `{name}` cannot become a stored-template placeholder.

## Dataset and temporal scope

Default queries retain Quipu's ROOT default. Use `from_graph`/`fromGraph` for
explicit FROM graphs and `from_named`/`fromNamed` with `Graph` for named graphs.
No builder method silently widens the dataset to every graph. The optional
`dataset` argument to `save` selects an existing stored-query catalog dataset;
FROM clauses remain part of the standard query text.

`valid_at`/`validAt` accepts a UTC RFC3339 timestamp ending in Z. `as_of_tx`/
`asOfTx` becomes the existing request field `tx`. These transport options are
separate from portable SPARQL; `to_request`/`toRequest` retains both. Printing
only `to_sparql`/`toSparql` returns the portable text, without the temporal pins.

The stored registry currently has no temporal-definition fields. Saving a
pinned query therefore refuses before any write. Call `without_temporal()` or
`withoutTemporal()` explicitly to save the unpinned template, or execute the
pinned request directly. This prevents a save from silently changing its time
scope.
