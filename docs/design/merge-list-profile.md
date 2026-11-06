# Explicit list profile admission

Status: S5 source checkpoint; adapter integration and qualification remain open.

A list slot carries a whole element sequence. Its cells are representation,
not independently mergeable values. `merge_lists::admission` accepts an explicit
JSON profile naming concrete slots and their `whole` or `sequence` mode:

```json
{
  "schema": "quipu-list-profile/v1",
  "shapes_sha256": "<SHA-256 of exact shapes document bytes>",
  "slots": [
    {"subject": "<urn:owner>", "predicate": "urn:list", "mode": "whole"}
  ]
}
```

This is a caller-supplied merge profile, not a new RDF shape vocabulary.
Cardinality, labels and `sh:group` do not declare a list mode. The caller must
obtain the profile through its existing admission boundary; merely finding an
RDF collection in data does not authorize list treatment.

Admission verifies exact shapes bytes, parses the shapes as Turtle, validates
the subject resource and absolute predicate IRI, and rejects duplicate slots,
unknown fields/modes/schema and bounded-input overflow. Subject rendering is
normalized before duplicate detection. The returned profile hash covers the
original JSON bytes, so an adapter must include that hash in its merge identity
and reject a changed profile during replay or resume.

Admission does not establish that the caller is authorized to declare those
slots, or derive target-scoped declarations from SHACL. That binding remains an
adapter obligation. There is currently no production caller of this module;
the existing share, fork and pendant adapters are not silently opted in.

After admission, the typed reader validates every declared list and rejects
orphan/shared/annotated/cyclic cells and unidentified nested blank-node values.
Lowering applies the same nested-value boundary, including no-op plans. A literal
whose lexical value begins with `_:`, however, remains an ordinary literal.

Remaining work: adapter input and identity binding; targeted shape/profile
authorization; complete-list operation grouping; list-divergent presentation;
qualification of this profile module and its negative controls. The existing
fixture-fix qualification is a separate exact source checkpoint.
