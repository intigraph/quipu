# Lazy deduplication for explicitly limited triple scans

`SELECT ?s WHERE { ?s ?p ?o } LIMIT 1` previously stopped the Rust row
collector after one binding, but its SQLite statement first grouped the whole
physical relation by canonical value key. The limit could not avoid that work.

An explicitly limited single leaf now reads physical rows lazily and uses the
existing canonical triple set before counting each returned binding. Physical
duplicates, numeric identity equivalents and attached dictionary aliases do
not consume the limit. Graph predicates and canonical subject, predicate and
reference-object resolution remain on the existing path. No SQL LIMIT is added.

Eligibility requires an explicit Slice with a length, a Project/Reduced chain
ending in one BGP triple with independent variable endpoints and either a
fixed predicate or an independent predicate variable, an empty seed, and
current-time evaluation without inference or pushed string filters. Repeated
variables, joins, FILTER, DISTINCT, ORDER BY, GRAPH operators and temporal
queries retain their existing SQL. A service response ceiling alone does not
enable the optimization. Unbounded queries and streaming scalar COUNT retain
their grouping and memory behavior.

The eligibility marker is scoped to synchronous evaluation of that subtree.
Its guard restores the previous thread-local value after nesting, errors or
unwind. It does not change the public temporal context or request protocol.
Queries without ORDER BY have no ordering guarantee; a different physical
representative or limited subset is permitted. Result headers are unchanged.

The private native-evaluator regression uses 10,000 synthetic ROOT facts and
the query above. Before the change it observed 140,078 SQLite VM instructions;
afterwards it observed 30, returning one subject in both cases. It counts the
native evaluator directly because the public entrypoint installs its own
deadline progress handler. The separate public-query controls compare canonical
row multisets and headers for duplicates, numeric equivalents, named graph
unions, missing graphs and attached aliases.

These measurements are not production timings or proof of a production
incident's cause. The query's parser, policy, admission, store acquisition and
unrelated concurrent work can have other costs. Runtime acceptance requires the
normal reviewed deployment and separately authorized measurements. Rollback is
a normal revert of the introducing merge.

A fixed `rdf:type` predicate with a variable object uses the same lazy leaf
path. A fixed class object retains the separate subclass expansion evaluator.
The typed canary regression uses 10,000 explicit type facts and counts native
SQLite instructions before the first result. It must return a real subject
without grouping the entire type relation. Full-result controls retain explicit
type semantics, class expansion, physical duplicate and numeric identity
deduplication, and default-graph unions. This is a bounded work control, not a
production latency claim.
