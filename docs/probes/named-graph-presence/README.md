# Limited named-graph presence probes

A single named-graph presence subquery can ask for one constant marker while
materializing all matching facts. The ordinary limited-leaf path did not admit
its fixed `GRAPH` and literal projection wrappers.

The new path admits only a current, unseeded single triple inside a fixed named
graph, projected as one literal marker. It retains canonical binding and
alias deduplication while stopping after the required solutions. The combined
preflight remains one query; no extra lazy request or cached absence is added.

Variable graphs, arbitrary result projections, filters, joins, ordering,
distinct results, volatile expressions, type entailment and historical contexts
retain the preceding evaluator. Existing deadlines and memory bounds remain.
This does not remove grouping globally or establish incident recovery.

## Reproduce

Build the library tests on the preceding and candidate sources. The preceding
binary needs the same test-only entry counter and reproducer; its product code
must remain unchanged. Retain that binary before building the candidate.

```sh
BEFORE_TEST_BINARY=/path/to/preceding-tests \
AFTER_TEST_BINARY=/path/to/candidate-tests \
bash docs/probes/named-graph-presence/run-demo.sh
```

The reproducer uses a disposable in-memory store with 3,000 facts in an ephemeral
named graph. Both sides verify exactly the `complete` and `eph_present` markers
and the projected header. The preceding test intentionally fails its new
eligibility assertion; the script checks that exact failure exit separately.
The counter records limited-scope entries; SQLite VM steps measure actual
statement work. Neither is a production performance measurement.

Negative eligibility controls and graph-isolation, empty, absent, numeric,
language-literal and dataset-exclusion answer controls accompany the probe.
Roll back with a normal revert of the actual merge commit.
