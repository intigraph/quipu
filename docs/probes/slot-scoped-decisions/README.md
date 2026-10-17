# Share decisions after unrelated writes

Compare an older CLI with the candidate without contacting a service or installing
anything:

```sh
python3 docs/probes/slot-scoped-decisions/run-demo.py --baseline /path/to/old/qp --candidate /path/to/candidate/qp
```

The same private fixture emits and decides one status conflict, then applies it
across six independent copies with zero to five unrelated writes. A second pair
of controls changes the reviewed slot or supplies a different valid incoming
share; both must refuse without changing the graph. Every accepted apply must
preserve the unrelated facts. Each binary is copied into its temporary HOME.

The 120–600 second cadence labels are synthetic; the script does not sleep or
measure a production refusal rate. It verifies the staleness boundary, not live
throughput. Source tests additionally cover legacy files, missing hashes,
value-identical reassertions with changed provenance, and new conflicts.

Recorded result: [output.json](output.json). Baseline accepted only the unchanged
arm (1/6); candidate accepted all six arms (6/6), preserving unrelated facts.
Both refused a changed reviewed slot and a different incoming share without
changing the graph. This is a private behavior comparison, not a production
refusal-rate measurement.
