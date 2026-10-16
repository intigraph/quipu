# Dense owner status guards

A selective owner relation can still contain thousands of subjects. Evaluating
its unchanged status guard once per subject repeats indexed reads, even when
almost every subject has a valid status. This change batches those subjects
through the ordinary canonical triple reader, then applies the same filters.

The planner admits current named-graph subject stars with `rdf:type`, a fixed
non-type property, constant predicates, and no incoming seed or string narrowing.
It retains the existing sparse planner through 128 selected rows. Batches cover
at most 16,384 selected rows, 128 canonical subjects per query, and 50,000 rows
per completed BGP relation (and per absence batch). An 8 MiB conservative
retained-binding accounting bound also covers keys and literal payloads; a single
decoded row is transient. This is not a total-RSS cap. Overflow restores the
existing planner; it never returns
a truncated relation. ROOT, historical reads, explicit entailment, variable
graphs, unsupported expressions, and unsafe scopes retain their prior paths.

For the existing grouped guard join, dense inputs reuse an already proven
subject-only driving relation. The trailing BGP may not introduce variables
absent from the guarded BGP. Extra driving bindings retain the preceding path.
Filters retain their original order. A plain-property absence test can batch its selected subjects
when the property variable has no outer binding. Other filters keep the existing
evaluator. Physical aliases, duplicate facts, named scope, and final bindings use
the shared triple reader. Speculative malformed rows fall back; deadlines and
query budgets still propagate.

Run the demonstration from the repository root:

```sh
bash docs/probes/dense-owner-status-guard/run-demo.sh
scriptreplay --log-timing docs/probes/dense-owner-status-guard/timing.log \
  --log-out docs/probes/dense-owner-status-guard/output.log
```

The fixture has 12,001 subjects across two named graphs, dense literal and IRI
ownership, and a deliberately invalid status. Every timed ASK
must return `true`, proving the invalid-status positive control remains visible.
The order is preceding planner, candidate, candidate, preceding planner. The
preceding planner is the prior per-subject implementation in the same test
binary, with only the new batching paths disabled. That bypass is compiled only
for tests. These synthetic timings are not production acceptance or total-memory
measurements. Existing result, intermediate-row, and deadline budgets remain.

The recording and its measured timings are linked in the pull request. Revert
the actual merge commit to roll back the source change.
