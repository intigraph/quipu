# SHACL Validation

> **Implementation status (2026-07-23, kelly):** ✅ **Implemented.** `src/shacl.rs`
> (backed by `rudof_lib`, a default feature) provides `Validator::from_turtle`/
> `validate`/`validate_or_reject` and `ValidationFeedback`/`ValidationIssue` exactly as
> documented; write-time enforcement is wired via `[quipu.shacl] validate_on_write`
> (`src/config.rs` → `src/server.rs`), with `/shapes` and `/validate` REST routes. No
> gaps found.

Quipu enforces strict schema at write time via
[SHACL](https://www.w3.org/TR/shacl/) (Shapes Constraint Language),
powered by [rudof](https://github.com/rudof-project/rudof).

## How It Works

1. Define SHACL shapes in Turtle format
2. Create a `Validator` from those shapes
3. Validate proposed data before writing to the fact log
4. Get structured feedback on failures

```rust
use quipu::{Validator, validate_shapes};

let shapes = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://example.org/> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

ex:PersonShape a sh:NodeShape ;
    sh:targetClass ex:Person ;
    sh:property [
        sh:path ex:name ;
        sh:datatype xsd:string ;
        sh:minCount 1 ;
    ] .
"#;

let data = r#"
@prefix ex: <http://example.org/> .
ex:alice a ex:Person ; ex:name "Alice" .
"#;

let feedback = validate_shapes(shapes, data).unwrap();
assert!(feedback.conforms);  // true -- data is valid
```

## Validation scope and pending post-state

The gates have different scopes. Do not describe all write validation as a
whole-graph pending-post-state check:

- `quipu knot --shapes` and `validate_shapes` validate the submitted data.
- Contextual write-time SHACL starts with that payload verdict. If it fails,
  a bounded second pass adds referenced types and required properties already
  stored in the destination graph and ROOT. It can remove payload violations;
  it cannot introduce new violations found only in that context. A conforming
  payload takes the fast path. This preserves incremental-write compatibility
  and is **not** whole-graph conformance certification.
- A violation that remains after that pass, on a node that already exists,
  is admitted as a `Warning` only if the node's stored pre-state (types plus
  required-path values) reports the identical issue: same focus, component,
  path, value, message, and named source shape. This is pre-existing debt, and
  it is counted through `shacl.debt_admitted` events. New nodes, new types, new
  values and other paths are refused. On `/update` under `validate_on_write`, a
  deletion that newly breaks a `sh:minCount` or `sh:hasValue` is refused.
  Known gap: a deletion that breaks a non-local constraint (`sh:or`/`and`/
  `xone`/`not`, `sh:node`, qualified counts) is not judged yet.
- The write-savepoint value gate applies the same rule to stored values: a
  value violation on a focus that existed before the transaction, and that its
  pre-transaction facts already showed, is admitted as debt and counted.
  `sh:maxCount` is never admitted, on either path: its result carries no
  value, so adding values to an over-count path would otherwise look like the
  old debt.
- Write-savepoint admission also checks the stored values of touched focuses
  for maximum count, datatype, node kind, class, pattern, length, numeric bounds,
  enumeration, language, and nested-node constraints. This catches a value written
  before the type or edge that targets it. Referenced-node context stays in the
  destination graph (ROOT contributes types only). Scalar constraints do not
  walk references; class checks add only referenced types, and nested-node
  checks follow declared direct-predicate node paths, not arbitrary links.
  Optional context has fact/node work budgets, not graph-size admission limits.
  Exhaustion discards partial value context and falls back to the pre-existing
  payload/maxCount path; it does not reject a valid write or certify its stored
  value constraints. Existing maxCount context limits still apply. These budgets
  are not measurements of the largest production graph. It does not turn context nodes into
  additional write-owned focuses. Changes to a child recheck its bounded
  incoming dependents on declared direct-predicate `sh:node` paths. Other incoming
  dependencies are not a whole-graph revalidation guarantee. This does not add a new minimum-count requirement to an
  incomplete incremental write. Existing deactivation and unsupported cardinality
  path behavior remain unchanged.
- With `governance.enforce_on_write` enabled, an applicable action-boundary
  policy evaluates its claim for touched targets against the transaction's
  pending post-state. A parent can require evidence on its linked child in the
  same transaction or already committed in the store. Evidence promised in a
  later transaction does not satisfy the current write: refusal rolls back
  the parent and link. This guarantee is for applicable touched targets,
  not automatic revalidation of every incoming dependency of a changed node.

`tests/pending_poststate.rs` pins the distinction with positive and negative
controls. A linked child's missing required name fails SHACL when the child
is typed in the submitted payload. If its incomplete type exists only in
store context, the contextual repair may accept a parent reference: the
new child violation is outside the original payload ceiling. Conversely, a
policy requiring linked evidence accepts a complete same-transaction write
and refuses the missing-evidence write before that evidence exists.

```sh
just test --locked --test pending_poststate
```

Use the policy guarantee for transactional cross-entity requirements. Use a
separate full-store or explicitly scoped validation for stored SHACL
conformance; incremental acceptance alone does not establish it. Neither
this documentation nor the tests enable a gate or change its runtime policy.

Library applications with an explicit multi-graph dataset may scope referenced
type lookup to that dataset for one operation:

```rust,ignore
store.with_write_reference_graphs(destination, &[destination, project], |store| {
    store.transact_graph_batches(&batches, timestamp, actor, source)
})?;
```

The graph IDs come from the application's configured dataset. This API performs
no sibling discovery and grants no write authority. It selects only referenced
nodes' `rdf:type` context for writes to the named destination, on both payload
repair and post-state validation. ROOT contributes referenced types only when
explicitly listed. Property values, cardinality and write-owned focuses stay
scoped to the destination. Other destinations keep their default context.
The prior scope is restored after a return, an error or unwinding, including
nested operations. The scope requires its destination and at most sixteen
graphs; it neither enables SHACL nor persists a configuration change.

## Agent-Friendly Feedback

When validation fails, the `ValidationFeedback` struct provides structured
details that agents can act on:

```rust
if !feedback.conforms {
    for issue in &feedback.results {
        println!("Severity: {}", issue.severity);
        println!("Focus node: {}", issue.focus_node);
        println!("Component: {}", issue.component);
        if let Some(path) = &issue.path {
            println!("Path: {}", path);
        }
        if let Some(msg) = &issue.message {
            println!("Message: {}", msg);
        }
    }
}
```

This is the core of Quipu's "strict but helpful" philosophy -- validation
doesn't just reject, it tells the agent exactly what's wrong and where.

## Supported Constraints

Through rudof, Quipu supports the full SHACL Core specification:

- **Cardinality**: `sh:minCount`, `sh:maxCount`
- **Value type**: `sh:datatype`, `sh:class`, `sh:nodeKind`
- **Value range**: `sh:minInclusive`, `sh:maxInclusive`, `sh:minExclusive`, `sh:maxExclusive`
- **String**: `sh:minLength`, `sh:maxLength`, `sh:pattern`
- **Property pair**: `sh:equals`, `sh:disjoint`, `sh:lessThan`
- **Logical**: `sh:and`, `sh:or`, `sh:not`, `sh:xone`
- **Shape-based**: `sh:node`, `sh:property`, `sh:qualifiedValueShape`
- **Other**: `sh:closed`, `sh:ignoredProperties`, `sh:hasValue`, `sh:in`

## Reusable Validators

Create a `Validator` once and validate multiple data payloads:

```rust
let validator = Validator::from_turtle(shapes)?;

// Validate multiple payloads
let result1 = validator.validate(data1.as_bytes())?;
let result2 = validator.validate(data2.as_bytes())?;

// Or use the convenience reject method
validator.validate_or_reject(data.as_bytes())?;
```

## Code Entity Shapes

Quipu ships SHACL shapes for Bobbin's code entity model in
`shapes/code-entities.ttl`. These enforce the knowledge-aware bundles
ontology at write time:

| Shape | Target Class | Required Properties |
|-------|-------------|-------------------|
| `CodeModuleShape` | `CodeModule` | filePath, repo, language |
| `CodeSymbolShape` | `CodeSymbol` | name, definedIn (class CodeModule) |
| `DocumentShape` | `Document` | filePath |
| `SectionShape` | `Section` | heading, headingDepth (integer) |
| `BundleShape` | `Bundle` | rdfs:label, contains (min 1) |

`CodeSymbolShape` constrains `symbolKind` to an enumerated set of values
(function, method, class, interface, enum, struct, variable, constant,
module, property, field, constructor, type\_alias).

`BundleShape` requires `contains` members to be instances of CodeModule,
CodeSymbol, Document, or Section.

Load the shapes via the REST API:

```bash
curl -s localhost:3030/shapes -X POST \
  -H "Content-Type: application/json" \
  -d "{\"action\": \"load\", \"name\": \"code-entities\", \
       \"turtle\": \"$(cat shapes/code-entities.ttl)\"}"
```
