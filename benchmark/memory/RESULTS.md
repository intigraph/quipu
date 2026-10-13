# ARM F results

Generated from real Dolt SQL-server runs, seeds 1, 7, 42, 99, 2026.
All counts below are per seed; the balanced case counts are identical by design.

| arm | units | false units | FP slots | FN slots | facts lost | facts extra |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Dolt JSON | 100 | 40 | 60 | 0 | 100 | 20 |
| Whole-cell TEXT | 140 | 80 | 140 | 0 | 180 | 60 |
| Typed rows + provenance | 100 | 40 | 40 | 0 | 40 | 20 |

Warm SQL latency: pooled median of 7 timed executions per seed (35 samples).
Each execution returns independently constructed expected answers; plans are in metrics.json.

| query | JSON median ms | rows median ms |
| --- | ---: | ---: |
| status | 7.825 | 1.227 |
| tag | 7.627 | 0.789 |
| traversal | 2.358 | 22.853 |

Dolt 2.1.8; every fixture stopped and its loopback port closed after the run.
Five invalid-type candidates per seed were rejected by the prototype writer before mutation;
native JSON accepted those same values. This is a writer guard, not native SHACL enforcement.

The TEXT arm is an ablation, not the behavior of current Dolt JSON. Native JSON already
merges independent object keys. Typed rows avoid concurrent set-addition conflicts, but
set_in_version makes identical semantic values with distinct provenance versions conflict.
Neither total native decisions nor entirely false native decisions decreases against JSON in this workload.
The slot counts expand a conflicted JSON/TEXT Memory to its changed metadata keys; they
do not claim Dolt exposes a distinct prompt for each JSON key. Loss includes deferred valid
changes under the common hold-at-base convention, not a claim of irreversible deletion.

Seeds change labels/order and query populations; they do not change the balanced ten-template
merge workload. Bodies are unchanged. The trial is synthetic and says nothing about production
conflict frequency. The prior 0/93 alias blindness, negative Arm D, and single-deployment limit stand.
