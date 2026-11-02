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
  into smaller migrations. Intermediate WHERE bindings also carry a row
  budget, so a filter or join may be refused even when its final result is
  small. A stricter native `max_join_rows` setting still takes precedence.

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

#### Blast-radius plan and risk gate

Before a proposal is accepted, Quipu plans what it touches and scores the risk.
The plan writes nothing. `quipu propose plan <id>` prints it (`--format json`
for the structured form), `quipu_list_proposals` with `"plan": true` attaches
it to each pending proposal, and `quipu_accept_proposal` returns it as `plan`.
It reports:

- `classes` and `properties`: the terms the proposed Turtle names (target and
  `sh:class` classes, `sh:path` properties, typed RDFS/OWL classes and
  properties, and the ends of `rdfs:subClassOf`, `rdfs:subPropertyOf`,
  `rdfs:domain` and `rdfs:range`);
- `removed_terms`: for a shape proposal, the terms the current version of the
  target shapes names and the proposal drops;
- `instances_in_scope` and `newly_failing`: the ROOT focus nodes the proposed
  shapes select (explicit targets and shapes that are also classes), and how
  many of them would newly fail SHACL. Each is validated under the current
  shapes and under the shapes as they would stand after acceptance; only
  violations the current shapes do not already report are counted, so
  existing debt is not blamed on the proposal. The counts are exact; at most
  20 issues are listed;
- `stored_queries`: current stored queries whose template names a changed
  term, as `<iri>` or as `prefix:local` under a `PREFIX` it declares;
- `named_graphs` and `snapshots`: named graphs whose data uses a changed
  property as a predicate or a class as a type, split into live graphs and
  frozen snapshots;
- `risk`: a 0-100 score, its threshold, the verdict and every input with its
  points.

| Input | Points |
|---|---|
| `newly_failing_instances` | 0 for none; 20 for 1-9, 30 for 10-99, 40 for 100-999, 50 for 1000 or more |
| `stored_queries` | 5 each, at most 15 |
| `named_graphs` | 2 each, at most 10 |
| `snapshots` | 5 each, at most 10 |
| `removed_terms` | 5 each, at most 10 |
| `migration_ops` | 2 each, at most 5 |

The verdict is `pass` when the score is at or under the threshold, `fail` when
it is above it, and `undetermined` when the plan cannot be computed honestly:
SHACL compiled out, a SPARQL-based `sh:target`, a diff that is not Turtle
(such as a JSON patch), or more than 50,000 focus nodes. A shape proposal
whose Turtle does not parse is not planned at accept; the accept refuses it
with its own message, as before. `quipu propose plan` exits 0, 1 and 2 for these. The threshold is 40
unless `QUIPU_PROPOSAL_RISK_THRESHOLD` sets another default, and a call can pass
`--risk-threshold` (CLI) or `risk_threshold` (MCP/REST).

Accept refuses a `fail` or `undetermined` plan, leaving the proposal pending,
unless the caller accepts the risk with a reason: `--accept-risk --reason
<why>` on the CLI, `"accept_risk": true, "risk_reason": "<why>"` over MCP and
REST. The decision note then records `accepted at risk <score>/<threshold>
(<verdict>): <why>`.

The plan covers ROOT instance data for the SHACL count. Named-graph data is
reported by graph, not validated, and a complex `sh:path` (inverse, sequence)
contributes no property to the term scan.

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

# Plan: blast radius and risk score, no write (exit 0 pass, 1 fail, 2 undetermined)
quipu propose plan 1

# Accept (refused if the plan fails, unless the risk is accepted with a reason)
quipu propose accept 1 --note "Approved"
quipu propose accept 1 --accept-risk --reason "owner reviewed the 3 instances"

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
