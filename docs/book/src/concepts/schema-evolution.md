# Schema Evolution

Quipu enforces strict ontology rules via SHACL shapes. When an agent writes
data that fails validation because the schema is too restrictive or missing a
class/property, it can **propose** a schema change instead of asking a human to
edit shapes manually.

## The Proposal Workflow

```text
Agent writes data → SHACL rejects it → Agent submits proposal → Approver accepts/rejects
```

Proposals follow a simple lifecycle: **pending → accepted** or **pending → rejected**.
Every proposal requires an explicit approver — there is no auto-accept.

## Proposal Kinds

| Kind       | Description                                    |
|------------|------------------------------------------------|
| `shape`    | New or updated SHACL shape (Turtle fragment)   |
| `ontology` | OWL axiom change (future)                      |
| `class`    | New RDF class definition                       |
| `property` | New or modified property definition             |

## MCP Tools

### Submit a Proposal

```json
{
  "kind": "shape",
  "target": "PersonShape",
  "diff": "@prefix sh: ... ex:PersonShape a sh:NodeShape ; ...",
  "rationale": "Need email property for contact info",
  "proposer": "agent/data-enricher",
  "trigger_ref": "validation-failure-42"
}
```

Tool: `quipu_propose_schema_change`

### List Proposals

```json
{ "status": "pending" }
```

Tool: `quipu_list_proposals`

### Accept a Proposal

```json
{ "id": 1, "decided_by": "reviewer-1", "note": "Looks good" }
```

Tool: `quipu_accept_proposal`

When a **shape** proposal is accepted, Quipu:

1. Validates the Turtle diff is syntactically correct
2. Verifies it parses as valid SHACL
3. Writes the shape to the `shapes` table
4. Records the approver and timestamp

If the Turtle is invalid, the proposal stays **pending** and an error is returned.

### Data Migrations

Changing a shape does not change the facts already stored. Renaming
`ex:name` to `ex:fullName` would otherwise leave every existing person with a
value the new shape ignores and without the one it requires. A proposal can
therefore carry a `migration`: an ordered list of operations, each of which
Quipu renders to a SPARQL Update template.

| `op` | Fields | Effect |
|------|--------|--------|
| `rename_predicate` | `from`, `to` | every `?s <from> ?o` becomes `?s <to> ?o` |
| `retype_class` | `from`, `to` | every `?s a <from>` becomes `?s a <to>` |
| `cast_datatype` | `predicate`, `datatype` | each literal value becomes `STRDT(STR(?o), <datatype>)` |
| `split_property` | `from`, `into: [{predicate, expression}]` | each value `?o` becomes one value per target, computed by a SPARQL expression over `?o` |
| `merge_property` | `from: [..]`, `to`, optional `expression` | values move to `to`; with `expression`, the sources' values `?v0`, `?v1`, … combine into one |
| `sparql` | `update` | a raw `DELETE/INSERT … WHERE`, `INSERT DATA` or `DELETE DATA` |

```json
{
  "kind": "shape",
  "target": "people",
  "diff": "... sh:property [ sh:path ex:fullName ; sh:minCount 1 ] ...",
  "proposer": "agent/modeller",
  "migration": [
    { "op": "rename_predicate", "from": "http://example.org/name", "to": "http://example.org/fullName" },
    { "op": "cast_datatype", "predicate": "http://example.org/age", "datatype": "http://www.w3.org/2001/XMLSchema#integer" }
  ]
}
```

The program is checked when the proposal is submitted:

- templates read and write the ROOT graph only, so `GRAPH`, `SERVICE`, `USING`
  and `WITH` are refused, including inside `EXISTS`;
- each `expression` must parse as exactly one SPARQL expression with no
  `EXISTS`, and the template uses the parsed form, never the submitted text;
- a migration whose `WHERE` matches, or whose program materializes, more than
  `server.update_full_copy_max_facts` facts is refused at accept and in a dry
  run. The shared program budget counts deletes and inserts before netting,
  including duplicate facts and facts that cancel out. Split larger programs
  into smaller migrations.

On accept:

1. Every operation is evaluated against the state before the migration,
   on the ROOT graph. The program's net change is
   `(before − deletes) ∪ inserts`.
2. The proposed shapes are loaded, the decision is recorded, and the change set
   passes the write gate under the **new** shapes and vocabulary.
3. The change commits as **one transaction** with source `migration:<id>`.
   Old facts are superseded: their valid time closes at the decision instant
   and they are never rewritten, so an as-of read before the migration still
   returns them.

If any step fails, such as an ill-typed cast like `"abc"` to `xsd:integer` or a
value the new shapes refuse, the proposal stays pending, the shapes are
unchanged and no fact is written.

#### Dry run

`quipu_accept_proposal` with `"dry_run": true` accepts nothing and reports:

- `refused_before_migration`: violations the new shapes find in the facts
  stored today;
- `migration`: per operation, how many facts it supersedes and writes, plus
  samples;
- `refused_after_migration`: violations left once the migration has run;
- `gate.would_accept`: whether the acceptance would go through, with the reason
  when it would not.

### Reject a Proposal

```json
{ "id": 1, "note": "Too permissive — would allow unconstrained strings" }
```

Tool: `quipu_reject_proposal`

## CLI

```bash
# List all pending proposals
quipu propose list --status pending

# Submit a proposal from a Turtle file
quipu propose submit shape PersonShape shape.ttl --proposer agent/enricher

# Accept
quipu propose accept 1 --note "Approved"

# Reject
quipu propose reject 1 --note "Needs tighter cardinality"
```

## Validation Hints

When `quipu_knot` rejects data due to SHACL violations, the response includes a
`hint` field pointing to `quipu_propose_schema_change`. When
`validate_or_reject` fails, the error message includes the same hint. This gives
agents a clear remediation path instead of a dead-end error.

## Design Notes

- The `proposals` table uses a text `diff` column so that SHACL shape diffs and
  future OWL axiom diffs can coexist. The `kind` column disambiguates.
- Proposals without a migration only mutate shape/ontology definitions. A
  migration writes facts only through the normal write path, in one
  transaction attributed to `migration:<id>`.
- Over MCP and REST there is no default approver: accept and reject refuse a
  request without `decided_by`. The `quipu propose accept|reject` CLI takes
  `--decided-by <id>` (or `--decided-by=<id>`); without the flag the CLI records
  `cli:<QUIPU_AGENT or USER>`, and with neither set it refuses rather than record an
  anonymous decision. A flag given with no usable value also refuses; it never falls
  back to the environment. A capability-based
  authorization system (`quipu.schema.approve`) is a future concern.
