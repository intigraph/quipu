# Stable labeled communities

`project --algorithm louvain` returns deterministic structural communities with
stable IRIs, a short data-derived label, and `label_basis`. No model or external
label service is involved. The selected graph supplies all label facts.
Unlabeled entities fall back to their IRI local names. Communities are not an
authorization boundary.

Identity hashes the projection scope and sorted member IRIs. After an opt-in
persisted run, a later run reuses identities through deterministic one-to-one
Jaccard matching (default threshold 0.5). Splits retain an old identity in at most
one child. Merges retain at most one parent and record other parents as lineage.
Matching ties use overlap, old IRI, then sorted new members. Filter and graph
scopes have separate identities.

`--persist` explicitly writes a producer-owned derived named graph, returned as
`derived_graph`; ROOT is unchanged. The graph is stable per projection scope,
while every run has a provenance IRI, source transaction, timestamp and parameters.
Reconciliation retracts stale current values; the store retains their history.
A foreign assertion in that graph refuses reconciliation. Graph creation and
fact publication are one transaction. `--timestamp` and `--match-threshold` can
make controlled fixture runs reproducible. The optional vocabulary is
`shapes/derived-groups.ttl`; loading it remains a separate operation.

Packs must explicitly include the derived graph. Consumers key labels by
community IRI, never array position. The additive Wasm `projectCommunities`
method returns the same descriptions but refuses persistence. It never writes
runtime community labels on behalf of the explorer. A pack without community
labels must keep its unlabeled fallback explicit.

Production graph-label governance and performance acceptance remain separate
from this source feature; no shape load or production persistence is implicit.
