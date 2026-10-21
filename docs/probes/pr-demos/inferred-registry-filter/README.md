# Inferred companion membership: controlled source recording

The fixture registers an asserted graph and its reserved inferred companion,
with one fact in each. Implicit membership must exclude the companion; explicit
`GRAPH` and `FROM NAMED` must still reach it. A graph-name string filter must
not opt into inference, while explicitly naming both graphs retains both.

[Terminal output](output.log) and [timing](timing.log) are a real, replayable
recording of the two compiled test executables invoked by [run.py](run.py).
Replay from this directory:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The BEFORE executable is a controlled counterfactual of the initial refreshed
filter candidate on base `9e24565f426a7ec902c11a897b2dbe55b1d43f6a`:
only its SQL-leaf exclusion is disabled. The AFTER executable has all exclusions
and the registry-first join cost repair. BEFORE reports one pass and one expected
failure (two rows instead of one); AFTER reports two passes. This demonstrates
membership on disposable in-memory fixtures, not deployment. Join read cost is
measured separately by the existing all-graph-search regression.

Executable SHA-256 identities:

- BEFORE: `df8661eabdebcc0ab9ef3d77c7ee26ffbb6c6f9a0e2a2fb395889a9b59e78932`
- AFTER: `24a8f03f14dc82b35a8fde7b2b14ec243e940902b964c1b445f77e4658b86b17`

To repeat, build the `smdlnm_` library tests in two isolated checkouts. In the
counterfactual, remove only `conditions.push(store.inferred_graph_exclusion_sql())`
from the unrestricted `AnyNamed` SQL branch. Build the unchanged candidate in
another checkout. Set `DEMO_BEFORE` and `DEMO_AFTER` to those test executables,
then run `python3 run.py`. Do not modify an installed service or shared checkout.

The small recording and caption are retained here for this PR. No external
upload, production graph creation, registry repair, or graph relabeling occurred.
