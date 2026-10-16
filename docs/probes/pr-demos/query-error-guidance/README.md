# Actionable query and vocabulary refusals

A missing `rdfs:` declaration still fails; it now names the undeclared prefix
and asks for an explicit declaration. No namespace is injected. Explicit custom
prefixes and valid queries retain their semantics.

An unknown class still refuses the whole MCP tool write with no facts written.
When a sufficiently similar local class name exists in the loaded vocabulary,
the error lists at most three class IRIs. Similarity is lexical, not semantic
equivalence: inspect the shapes and propose a new class when none fits. Empty
or dissimilar vocabularies produce no invented candidates.

Query-timeout advice points exploratory text discovery toward `quipu_search`.
That does not replace exact-ID, owner-plate, named-graph or absence checks, and
this change does not change query budgets or optimize the planner.

Reproduce the private native controls:

```sh
cargo test --locked --features full --lib sparql_structure::diagnostics -- --nocapture
cargo test --locked --features full --lib vocabulary::suggestions -- --nocapture
```

The vocabulary test calls the actual MCP tool handlers, checks that the refused
subject is absent, and successfully writes and reads a legal-type positive.
CLI `knot` uses a different path and is not an instrument for that refusal.
These are private fixtures, not a replay of a running graph or a claim of a
lower observed client failure rate. The new native MCP descriptions propagate
only when that server/tool catalog refreshes; proxy tool descriptions have
separate ownership.

[Actual response transcript](transcript.txt) compares the same private CLI inputs
on the installed before binary and a clean source-built candidate; the native
MCP class controls are labeled separately. This is a text fallback, not video,
and does not prove Mac adoption. The before file is a different source revision;
no runtime service was changed.
