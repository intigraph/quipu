# Lazy canonical deduplication for speculative FILTER relations

The FILTER driver can scan a complete predicate relation before evaluating a
stable conjunct and probing the other BGP patterns. Its retained triple visitor
already canonicalizes and deduplicates each row. Grouping that physical relation
in SQLite first repeats work before the first binding reaches the filter.

The proposed collector scopes an ungrouped retained scan to that speculative
relation only. The visitor still resolves aliases, applies graph and temporal
conditions, deduplicates canonical triple keys, and counts successfully bound
rows before enforcing the existing relation cap. No SQL LIMIT is introduced.
If the relation exceeds the existing maximum, the driver declines and the
ordinary evaluator runs. The scope restores its previous state on return,
error, nesting and unwind.

Ordinary triple leaves retain their SQL, including the separate explicitly
limited-leaf optimization. Scalar and streamed COUNT keep grouping rather than
retaining an unbounded canonical set. The FILTER driver's eligibility,
selectivity tests, deferred-error behavior, join logic, ordering and budgets
remain unchanged. Existing graph membership and attached dictionary resolution
use the same retained visitor.

Queries without ORDER BY have no scan-order guarantee. Complete relation parity
and ordered whole-query controls are required; a lower SQLite instruction count
alone does not establish a faster query. The qualification uses synthetic local
stores and an exact grouped reference, including duplicates, repeated variables,
seeds, graph unions, attached aliases and scope cleanup. It also compares the
full six-pattern time-window query in alternating baseline/candidate order.
These are private fixture measurements, not production latency or p95 evidence.

Rollback is a normal revert of the introducing source change. Runtime schema,
query, producer and consumer admission is separate from source qualification.
