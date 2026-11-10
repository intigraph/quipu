# Fluent contract v1

This is the source contract for implementing fluents. It is not permission to
activate a served schema, reinterpret existing slots, migrate data, or discard
history. `shapes/fluents-v1.ttl` versions pending proposal 52, the stable-IRI
successor to proposal 51. Proposal 51 remains unchanged as historical evidence.
The successor retains its pending-proposal warning. `shapes/MANIFEST` excludes it from
deployment loading. The warning describes live admission; versioning the draft
does not accept the proposal in a running store.

## Names and declaration

The canonical prefix for this new vocabulary is
`qp: <https://w3id.org/intigraph/quipu/ns#>`, the accepted stable Quipu vocabulary
identity. New Fluent, FluentObservation, Observable and associated property/shape
IRIs use that prefix. Implementations must use a dedicated fluent vocabulary
constant rather than minting these terms from the legacy `namespace::QUIPU`.
This does not rename that existing constant, migrate its terms or stored data,
admit aliases, or substitute a generic Observation for FluentObservation.
The registry name for a future activation is `quipu-fluents-v1`.

| Class | Required fields, exactly one unless specified |
| --- | --- |
| `qp:Fluent` | `slotSubject`, `slotPredicate`, `inGraph`, `propertyShape`, `currentValuePolicy` |
| `qp:FluentObservation` | `forFluent`, `originTx`, `writer`, `asOf`, `recordedOn`, `observationValue` |
| `qp:Observable` | All Fluent fields, one or more `observes`, one `computedBy` |

Fluent, observation, writer, subject, predicate, graph, property-shape and
dependency identities are IRIs. An observation's subject is its immutable event
identity. Observables explicitly carry both `qp:Observable` and `qp:Fluent` types;
writers must not depend on a reader silently materializing subclass types.

The named property shape has one IRI `sh:path` and one plain `xsd:string`
`qp:currentValuePolicy`: `latest`, `max`, `sum`, or `set`. Resolve its actual
targets for the selected graph and subject before admitting a fluent. The
Fluent's predicate and policy must match that shape. A predicate annotation is
not a global opt-in. Multiple policies, unknown policies, language-tagged policy
strings and complex paths refuse; missing annotations preserve ordinary slots.

The Fluent identity fixes graph, subject, predicate, property-shape identity and
policy. Changing their interpretation requires a new explicitly admitted
declaration; it must not reinterpret an existing observation log. Ordinary
status and assignee disagreements remain slot conflicts and decisions.

## Values and ordering

| Policy | Observation domain | Current value |
| --- | --- | --- |
| `latest` | RDF IRI or literal | Value of the greatest source-order event |
| `max` | `xsd:integer` in signed 64-bit range | Maximum integer; retain every tied producing event |
| `sum` | `xsd:integer` in signed 64-bit range, representing a delta | Exact sum of distinct events; refuse a result outside signed 64-bit range |
| `set` | RDF IRI or literal | Set of distinct RDF terms; retain all contributing events |

`sum` never means summing absolute counter snapshots. A producer converting
snapshots into deltas needs its own reviewed conversion and replay contract.
Numeric policies reject strings, decimals and out-of-range integers; no silent
coercion, saturation or floating-point reduction is allowed. The writer checks
this cross-record policy constraint; the structural shape alone does not.

Latest/set preserve datatype, language, lexical form and IRI identity. A numeric
literal and a string with the same characters are different terms. Blank nodes
are not portable event values in v1; a producer must use an admitted stable IRI.
Triple terms are reserved for the projection reification path below, not an
implicit escape from observation-value validation.

`originTx` is a positive source transaction assigned under the native writer
reservation; `writer` is the authenticated source identity, not caller-selected
authority. Imported observations retain both fields with verified source
provenance. Import time is not source order. Order is the tuple
`(originTx, writer IRI, observation IRI)`, never host-clock order. Transaction
numbers are source-local: the tie-break is deterministic, not a claim of global
causal ordering between stores.

Merge is union by event identity. Exact replay is a no-op; the same identity
with different content refuses atomically. Two distinct events with equal
values are still two contributions to `sum`. Writers must refuse replacement,
retraction and identity/content mutation through every ordinary write adapter,
not just a new append API.

## Time, projections and observables

`asOf` is domain valid time; `recordedOn` is source recording time. Both are
timezone-qualified date-times; native writers use UTC. Keep both on immutable
events and retain them across transport. Transaction-order intervals are not a
substitute for a domain valid-time reader. Clock-skew controls must demonstrate
that domain timestamps do not select the ordinary current-value winner.

The current projection must be graph-scoped, replaceable and reconstructible
from events. Reification uses the standard RDF 1.2 `rdf:reifies` predicate: an
IRI reifies the projected `(subject, predicate, value)` triple term and links
to its producing observation IRIs with `prov:wasDerivedFrom`. For `sum`, this
describes the aggregate projection, not a false claim that each delta event
asserted that aggregate. `max` retains ties; `sum` and `set` retain contributors.
No additional projection class or executable policy expression is introduced.

The [RDF 1.2 Concepts snapshot](https://www.w3.org/TR/2026/CR-rdf12-concepts-20260407/)
defines triple terms and reification. Native parser, storage, validation and
reader round-trip support must be qualified before claiming this projection
path. A string containing triple syntax or a generic Observation is not a
replacement. An unsupported triple term must refuse, not weaken ordinary RDF
parsing. Domain valid-time selection remains an implementation acceptance gate.

Observables register dependencies within their graph and use only the four
named computation policies. Registration rejects direct/transitive cycles,
unknown dependencies and cross-graph references atomically. `computedBy` and
`currentValuePolicy` agree. Evaluation is bounded, memoizes shared dependencies,
refuses missing inputs, and preserves original event provenance. This does not
admit arbitrary code execution or turn sum-of-deltas into sum-of-snapshots.

## Activation and implementation gates

SHACL covers structural fields and cardinalities only. It does not establish
immutability, source authenticity, target resolution, numeric compatibility,
cycle freedom, graph authorization, transport completeness, valid-time query
semantics, or retention safety. The owning writer implementation must enforce
these before the profile can be activated.

Keep raw immutable events until separately reviewed retention and roll-up
proofs establish deduplication across overlapping replicas. A cached roll-up
must not erase identities needed for replay or double-count overlapping input.
No raw-history deletion is authorized by this contract.

Before live proposal acceptance or changing the manifest to `load`, require:

- Native Store ownership, authority, SHACL and policy admission with no partial
  append on refusal; all replacement/retraction paths preserve immutability.
- Restart, duplicate-delivery, reversed-merge, reused-identity, clock-skew,
  numeric-boundary, graph-isolation and ordinary-slot-conflict controls.
- Persistent share/fork/pendant round trips preserving event identities and
  authenticated source order, plus domain-time and RDF 1.2 reader controls.
- Bounded observable evaluation and cycle/unknown-input refusal controls.
- Explicit retention/roll-up disposition, exact versioned shape bytes, the
  deployment classification and a separate schema-activation decision.

Until then, source-contract review unblocks implementation work only. It does
not certify the private algebra kernel or any installed fluent feature.
