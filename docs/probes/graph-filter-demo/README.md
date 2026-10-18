# Graph-name FILTER pruning

`GRAPH ?g { ?s a ?t } FILTER(CONTAINS(STR(?g),"seeds"))` formerly evaluated every
named graph before filtering rows. The candidate intersects the active named
dataset with matching graph names before the triple scans, and retains the real
FILTER. Only one recognized string test directly wrapping GRAPH-variable with a
plain BGP is admitted. Nested GRAPH, subqueries and complex expressions keep the
original path so the restriction cannot escape into another graph scope.

The recording runs the actual library evaluator on a synthetic fixture. It
compares sorted result bags and grouped counts with an equivalent unpruned
`FILTER((CONTAINS(STR(?g),"seeds")) || false)` control, and prints the number of
actual graph-leaf evaluations. This measures graph scopes, not every SQLite row
visit. No live service, embedding provider or production timing is demonstrated.

Run from the repository root:

```sh
bash docs/probes/graph-filter-demo/run.sh
```

The output/timing pair is a terminal recording retained beside the source. It
requires download and a terminal replay tool:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Recorded runtime source: `12861b9efb451cd765877e2930f17d28c793a46f` (integration base
`ee35d837897bbfe75061234bad725427d851a37c`; original source `a3be2710`).
The five proposal SPARQL blobs/modes are unchanged by that base integration.
The recording's fixture has 26 ordinary named graphs plus one Unicode control;
actual leaf evaluations fall from27 to7 and all compared result bags/counts match.
The control uses the same expression OR false to retain the old evaluator.

A separate same-input CLI experiment on a retained26-graph/800-types-per-graph
fixture measured filtered mean463.14 ->219.37ms/p95580.76 ->334.10ms in debug
builds including process startup. Seven samples per shape; control timing varies
with load. Before/after outputs for filtered, equivalent unpruned and all-graphs
queries are identical. These private figures are not served latency or proof of
the production target. Removing the plain-BGP guard deliberately breaks the
nested-graph differential test; exact restoration returns the suite to green.

[Output recording](output.log) and [timing](timing.log) are retained in this
small source directory. Playback requires a terminal replay tool, not an inline
video player. Reproduction and captions apply to the integrated source; no
installed-service claim follows from this recording.
