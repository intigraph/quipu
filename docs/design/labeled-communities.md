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

An opt-in companion producer is `scripts/build-community-companion.sh`. It takes
an already-scrubbed repository share and an explicit reviewed producer-policy
file, derives only in an owned temporary store, then exports a separate share
bound to the original `parent_share`. It never publishes or reads a live store.
The policy file must have the freshness/authority required by the release lane;
a public example catalogue is only suitable for synthetic fixtures.

The Wasm `loadCommunityPendant` receiver requires explicit shape adoption and an
expected graph hash, checks the parent against the loaded repository pack, and
uses the standard archive verifier and import gates. Facts remain in the returned
named staging graph. It does not promote them into ROOT. `source_scope` retains
the producer's derived graph identity; `derived_graph` is the actual receiver
scope to query. Producer authority remains UNKNOWN. Loading and integrity checks
are not a producer attestation.

`node scripts/test-community-companion.cjs <native-quipu> <node-bindings.js>`
exercises a native producer and the real Wasm receiver, including missing-adoption,
hash and parent refusals, cold-load identity and ROOT invariance. These fixtures
have their own policy and do not certify published pack adoption.

A private debug-build profile over 4,000 nodes/500 communities measured unchanged
resolve-only partition output at 34–41 ms and labeled descriptions at 335–353 ms.
Labels require PageRank and source-fact reads; that cost is explicit. This is not
a production latency or memory acceptance and does not clear the production soak.
