# ARM E: merge shapes before data

This is a **research orchestration** using the actual library share-merge API
twice, not a deployed schema-negotiation feature. Read [RESULTS.md](RESULTS.md).

```bash
just bench schema --selftest
just bench schema > /tmp/schema-results.json
just bench schema-report /tmp/schema-results.json
```

Each fixture creates separate exact base/left/right graphs in in-memory stores,
then real producer-built share envelopes with a common-base parent. Edits are
not appended to the base. Bundles stay in a run-scoped temporary directory with
an internal destination; no outward share is made. Public evidence contains
only synthetic RDF and is covered separately by the publication scrub gate.

First merge the RDF shape graphs under a fixed SHACL-SHACL meta-shape declaring
`sh:path`, `sh:maxCount` and `sh:datatype` functional on PropertyShape nodes.
Validate that merged graph against the meta-shape. Then merge the data using
the resulting shape contract and separately validate that output. All shapes
being merged use stable named nodes. Blank-node alignment, arbitrary SHACL
constraint interactions and meta-shape evolution remain outside this trial.

## Fixtures and oracle

Five explicit fixtures, repeated with identities 1, 7, 42, 99 and 2026:

| case | change | expected semantic decision |
| --- | --- | --- |
| tighten | maxCount 2 to 1 | two combined values now conflict |
| loosen | maxCount 1 removed | distinct replacements now coexist |
| property-added | new functional tag property | concurrent tag assertions conflict |
| datatype | string to integer, cardinality remains 2 | old-contract string requires a decision |
| removed | value shape removed, note shape retained | distinct value replacements coexist |

Left edits data under the base contract; right edits under its changed contract.
The driver checks each branch is valid under its own contract. The datatype case
starts with an empty value slot, so its base remains legal under either contract.
An incompatible left string and right integer are both individually legal but
not jointly legal under the new integer contract.

The oracle and expected normalized outputs are explicit fixture declarations.
The actual cardinality detector receives only the three graphs and resulting
SHACL. A datatype oracle conflict missed by that detector is counted as a false
negative even when post-merge validation catches the invalid output. Never add
the validator's rejection to detector recall. That case is a negative result
for using cardinality conflict detection alone after schema evolution.

## Controls and accounting

The meta-conflict positive control edits the same bound from 1 to 2 and 3 in the
two branches, requires one real conflict and holds that meta-slot at base.
The meta-validator positive control inserts two bounds and must reject it; the
single-bound held graph must conform. Legal branch controls, the tightened and
added-property conflict controls, the loosened/removed no-conflict controls and
the datatype violation control run through the actual library/validator.

The shipped merge blocks the entire transaction on conflicts. The scorer
normalizes the tested conflicting slot to base only to match Arm A's automatic
output convention. It does not claim the library applies uncontended data
partially. These small fixtures change no other data slots concurrently, so
that normalization has no hidden accepted changes to synthesize. Lost/extra
triples are computed against the declared held-slot oracle; the datatype case
has two extra unresolved values. A post-validation gate must reject that
candidate rather than admit it.

The Context Merge comparator counts changed predicate slots on subjects touched
by both branches, matching Arm A's context-overlap rule. Every edited object
here is a literal, so there are no changed object-node endpoints. It is a
reference count for these fixtures, not a run of Quit Store itself.

All source and result graphs are in [results.json](results.json). Repetition
changes entity identities, not statistical workload samples. No result extends
the earlier 0/93 alias blindness, negative Arm D or single-deployment evidence.
