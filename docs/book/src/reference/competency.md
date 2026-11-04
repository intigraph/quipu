# Pendant competency contracts

A pendant can describe what it answers and records in `competency.toml`.
The offline authoring API accepts schema `quipu-competency/1`, with only
`[[question]]` and `[[command]]` blocks. Unknown fields, unknown block kinds,
duplicate IDs and unsupported schema versions are errors.

```sh
quipu competency lint ./pendant
```

Lint reads the contract and, when a question uses `query_ref`, the pendant's
`queries.ttl`. It parses every definition, including retired ones, without
opening a database, contacting a server or executing a command. Success means
schema and syntax passed. It does not establish that a query returns the
expected data, that linked shapes exist, or that an example satisfies SHACL.

```toml
schema = "quipu-competency/1"

[[question]]
id = "people"
ask = "Which people are recorded?"
query = "SELECT ?person WHERE { ?person a <urn:example:Person> }"
returns = { person = "iri" }
justifies = ["urn:example:PersonShape"]
expect = "nonempty"

[[command]]
id = "record-person"
does = "Record a person's name"
inputs = { person = "iri", name = "string" }
template = '<{person}> a <urn:example:Person> ; <urn:example:name> "{name}" .'
satisfies = ["urn:example:PersonShape"]
write_class = "example-class"
key = ["person"]
example = { person = "urn:example:alice", name = "Alice" }
```

IDs contain lowercase ASCII letters, digits and internal hyphens. They are
unique across both block kinds. `ask`, `does` and `write_class` must be nonempty.
`write_class` is arbitrary data; lint assigns no grants.

Inputs and SELECT return columns use scalar types `iri`, `string`, `int`,
`integer`, `decimal`, `boolean`, `date` and `dateTime`. An input type ending
in `?` is optional. Templates supply the RDF/SPARQL delimiters around exact
`{name}` placeholders, as in `<{person}>` and `"{name}"`. Every placeholder
must be declared, and every declared input must occur in the template.
Syntax probes are stand-ins, never execution defaults. Example values are
strings; examples must cover required inputs and contain no undeclared inputs.
Example value types and actual rendered examples are checked by runtime
verification, not this syntax-only lint.

A question has exactly one of `query` or `query_ref`. A reference must resolve
exactly once in the pendant's stored-query member. SELECT declares exactly its
projected columns in `returns`; CONSTRUCT uses `returns_shape` and no column
map; ASK has neither. All questions require at least one `justifies` shape or
class IRI. `expect` may be `empty`, `nonempty` or `rows=N` for SELECT/CONSTRUCT,
or `ask=true` / `ask=false` for ASK. DESCRIBE and SPARQL Update questions are
refused.

Commands require nonempty `satisfies` shape/policy IRI lists, a `key` containing
unique required input names, and an `example`. Templates must parse as nonempty
Turtle or as SPARQL Update containing only nonempty INSERT DATA operations.
Lint checks their syntax; it does not grant authority or commit them.

Both block kinds accept `retired = true`. Retirement retains the definition.
An empty contract can pass syntax lint; a later execution verifier must report
that nothing was checked rather than count that as runtime success.

The Rust `quipu::competency` API provides `parse`, `load` and `Contract::compile`.
Compilation returns deterministic `queries.ttl` and `commands.ttl` strings,
sorted by identity and map key. It retains typed inputs, output contracts,
examples, expectations, target links and retirement state. Questions extend
the existing `StoredQuery` RDF vocabulary. Optional parameters retain their
optional flag without an invented default.

This authoring slice does not yet wire these members into share/import,
register generated tools, or execute commands. Its extended typed parameter
vocabulary requires a competency-aware consumer; it must not be registered
through the legacy stored-query loader as though runtime support were present.
