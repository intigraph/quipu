# My merge experiments: schema evolution and Memory metadata

Research results note, 2026-10-13. Counts are per seed; five identities repeat
balanced synthetic fixtures rather than sample production conflict rates.

**ARM F — real Dolt 2.1.8.** Each trial has 200 Memories and 60 true conflict slots.
Native JSON already merges independent keys; whole-cell TEXT is an ablation.
Typed rows union distinct set additions but create same-value provenance conflicts.

| representation | native decisions | entirely false decisions | collateral FP slots | missed slots |
| --- | ---: | ---: | ---: | ---: |
| json-native | 100 | 40 | 60 | 0 |
| whole-cell-text | 140 | 80 | 140 | 0 |
| typed-rows | 100 | 40 | 40 | 0 |

Rows reduce collateral slots, not native false decisions, in this workload.
Five invalid candidates per seed were refused by the prototype writer before mutation;
native JSON accepted them. A JSON writer can implement the same declaration guard.

**SQL probes.** Each seed has 2,000 Memories, 10,000 facts and 2,000 Links.
Every repeated answer matches independently constructed expectations; full plans/samples
are published. Pooled medians over 35 warm executions, in milliseconds:

| query | native JSON | typed rows |
| --- | ---: | ---: |
| status | 7.825 | 1.227 |
| tag | 7.627 | 0.789 |
| traversal | 2.358 | 22.853 |

**ARM E — actual library merges, shapes first.** Tighten, loosen, added property,
datatype change and removed shape each have five identity variants. A fixed meta-shape
merges named nodes before data; all source branches are individually valid.
Per seed, 2 merge decisions recover 2 oracle conflicts versus 5 Context questions;
1 datatype conflict is missed by cardinality detection and caught by validation.
Validation remains a gate. This is research orchestration, not deployed schema negotiation.

**Limits retained.** Body edits, ordered arrays, arbitrary SHACL and blank-node alignment
are untested. Timings use one fixture host and fixed query order. The 0/93 alias blindness,
negative Arm D and single-deployment evidence remain. Held-at-base loss counts deferred
valid changes, not irreversible deletion. Every Dolt fixture stopped.

Reproduction commands: benchmark/memory/README.md and benchmark/shapes/README.md.
Run `just bench merge-note` to regenerate this note from their published results.
