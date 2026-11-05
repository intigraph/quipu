# Shadow merge census: retrospective source stage

`scripts/merge-shadow-census.py` is a bounded, read-only analyser of an explicitly
retained Quipu SQLite snapshot. It never installs a writer hook, creates a
snapshot, changes admission, or changes the source database. Do not point this
source-stage tool at the live writer. An owner-bound retained snapshot and a
read-cost scope are required before collecting the production corpus.

```sh
just shadow-census inspect --snapshot /absolute/retained.sqlite \
  --after-tx 100 --through-tx 200 --max-pairs 5000 --gap-seconds 300
just shadow-census test
```

The analyser joins an old physical assertion's actual `retracted_tx` to a new,
different assertion of the same graph/entity/predicate in that transaction.
Both intervals must be nonempty when parsed as instants. Different declared
actors and a bounded transaction time gap select **candidates**, not proven
lost updates. Nearby writes without that closure link do not count. Legacy
closures without a recorded retracting transaction are unobservable, not zero.
The schema's retained-retraction and transaction indexes are required; the
analyser refuses an older schema rather than falling back to a full scan.

The explicit transaction window is `(after_tx, through_tx]` on replacement
transactions. Earlier assertions can contribute when their closure is in that
window. One SQLite read transaction binds store identity, log epoch, snapshot
head and rows. The default limit is 5,000 physical pairs, hard-capped at 50,000;
an extra row refuses the entire result. A five-second query/analysis deadline
(at most ten seconds through the library interface) also refuses. No partial
sample is advertised as a complete bounded result. CLI failures emit a static
`unknown` response without private paths or database diagnostics.

Output groups numeric graph and predicate IDs and counts candidate slot
transactions. It contains no entity IDs, fact values, actor strings or query
text. Physical source claims may duplicate one logical replacement; physical
pair counts explicitly do not claim logical-slot deduplication. Graph IDs do
not establish ontology membership. Multiple ontologies may share one graph.

This is **not** a second implementation of `merge3` and does not provide its
three-way classification. Read bases, shape/cardinality snapshots, class and
ontology mapping, authenticated actors and LWW-loss evidence remain unknown.
The live shadow stage must call the existing merge core against a declared
base, preserve normal write behavior, and bind its classification to the actual
shape/source version. It still needs reviewed source, normal qualification,
bounded owner-admitted collection, per-ontology reconciliation and coverage
evidence. Neither synthetic tests nor this source stage complete the census.
