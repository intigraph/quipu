# My merge experiments: schema evolution

Research results note, 2026-10-13. Counts are per seed; five identities repeat
balanced synthetic fixtures rather than sample production conflict rates.

**ARM E — actual library merges, shapes first.** Tighten, loosen, added property,
datatype change and removed shape each have five identity variants. A fixed meta-shape
merges named nodes before data; all source branches are individually valid.
Per seed, 2 merge decisions recover 2 oracle conflicts versus 5 Context questions;
1 datatype conflict is missed by cardinality detection and caught by validation.
Validation remains a gate. This is research orchestration, not deployed schema negotiation.

**Limits retained.** Arbitrary SHACL and blank-node alignment are untested.
The 0/93 alias blindness, negative Arm D and single-deployment evidence remain.
Held-at-base loss counts deferred
valid changes, not irreversible deletion.

Reproduction commands: benchmark/shapes/README.md.
Run `just bench merge-note` to regenerate this note from the published shape results.
