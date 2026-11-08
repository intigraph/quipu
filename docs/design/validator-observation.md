# Actual post-state validator observations

`Store::observe_validator` is an opt-in library scope around the normal writer.
It returns the operation's result and an ordered vector of gate observations.
It does not change admission, enable validation, commit a transaction, create a
merge operator, or install a production caller. An empty vector means no gate
execution was observed.

Each observation is made while the staged transaction still holds its writer
snapshot. Refused observations survive the subsequent rollback in memory. Their
coordinates are provisional writer coordinates: they are never a commit receipt.
A later guard or outer savepoint can still reject an otherwise successful gate.
Nested scopes contribute their observations to the parent and restore the
previous scope on unwind.

The snapshot binds loaded shape and ontology names, version strings and exact
bytes in registry order, the combined registry bytes, explicit write shapes
(`None` differs from `Some(empty)`), flags, and the destination's entire staged
asserted graph. That graph is read after normal write-time inference. Postcommit
materialization is a separately observed flag, never precommit evidence.

The executed selection branch records the loaded registry selection. The exact
policy-split and explicit shape bytes are retained by `effective_shapes()` and in
the manifest's `effective_shapes` implementation option. `checked_data()` retains
the exact encoder output passed to the actual validator, including order and
duplicates; the incremental DTO contains those same lines and its `checked_data` option
binds the complete encoder bytes. The complete staged
graph and the checked incremental context are distinct observations.

The reported context budgets use the same constants as the live guards.
Optional value-context exhaustion records `BudgetExhausted` and retains only the
truncated context that actually reached validation. A skipped, unexecuted,
unknown, or failed validation never produces `Complete` coverage. Raw SHACL
conformance and the gate's filtered ownership/debt decision are separate fields.
A successful gate is not evidence of full staged-graph SHACL conformance.

On Linux, hashing `/proc/self/exe` is supported only after executable code
mappings prove that both Quipu's extraction and its actual validator entry point
belong to that same device and inode. The proof also checks its own code image.
Path names, a readable host executable, and a library's presence in the process
are insufficient. Missing, malformed, oversized or foreign mappings refuse
manifest extraction. In particular a Rust `cdylib` consumer under Python is
unsupported: hashing Python would not identify the validator library.

The supported executable identity also binds the embedded dependency lock,
compiler-generated complete feature stamp, and explicit engine/parser/base/gate
options. It is not a source revision or deployment assertion. Non-Linux targets,
including Wasm, refuse extraction while loaded-image identity is unproved.
`just validator-observation embedding` exercises two different real Rust
shared-library consumers under one host. Their writes still succeed, but their
manifest extraction must refuse rather than report the unchanged host hash.

OWL cache-source identity is recorded when the actual cache is constructed and
cleared on explicit invalidation. An actual empty ontology lookup proves the
empty source for an absent cache. Registry edits do not retroactively change the
retained source identity. When a live cache is stale, the manifest therefore
contains its old source digest alongside the new registry bytes. Consumers must
preserve the resolver contract's existing downgrade for that mismatch.

Observation fields have no public constructor. `manifest()` exposes the public
resolver DTO only after the scope finished and capture succeeded. Its existing
identity and evidence checks still apply, including downgrade for exhausted or
unknown coverage, missing SHACL support and unproved OWL cache equivalence. The
consumer must separately prove candidate/source bindings, full prospective
validation, admission-time assignments and the existing authenticated receipt.

The scope is intentionally off by default: capturing whole graph bytes and
hashing an executable has a cost. Tests exercise real writer execution and
compare retained bytes with the actual validator invocation. Removing input
capture must fail those controls.

When the incremental gate invokes debt filtering, `debt_pre` and `debt_post`
implementation entries retain JSON objects with the actual shapes, data,
conformance and error from those validator calls. They are captured at execution,
not reconstructed by the observer. These entries and `checked_data` are evidence
bindings in the existing extensible manifest options; they are not configurable
validator settings. `context_exhaustion` records the actual exhaustion reason,
including the value-only outer fallback that deliberately skips validation.

The staged graph encoder is shared across builds, including those without the
SHACL engine. Missing validation support leaves coverage Unknown and preserves
the real staged bytes; it never substitutes an empty graph for missing evidence.

See the [execution transcript](../demos/validator-observation/observation.txt)
for actual source probes and observation-removal controls.

See the [loaded-image regression](../demos/validator-observation/linkage.txt)
for the failing-first shared-library control.
