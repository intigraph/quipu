# Inspect actual local query execution

Send a bespoke JSON `POST /query` request with `execution_profile: true`:

```json
{"query":"SELECT ?x WHERE { VALUES ?x { 1 2 } FILTER(?x = 1) }","execution_profile":true}
```

The normal answer is preserved, with an additional `execution_profile` object
using contract `quipu-execution-profile/v1`. The option is independent of
`verbose`, which controls IRI expansion. Omitted or false leaves the response
unchanged and starts no profiling clocks.

The profile reports actual evaluator work, rather than a predicted static plan:

- `operators` contains invocation counts, successful output-row sums, error
  counts and inclusive microseconds for algebra operator categories.
- `paths` contains aggregate counts for the actual BGP dispatcher, filter
  candidate selector and owner guard choices. `filter_status_candidates`,
  `filter_literal_exclusion`, `filter_driven` and `filter_recursive` distinguish
  candidate selection. `filter_output` reports actual candidate and surviving
  relation sizes, including queries with no Join nodes. `filter_join_dense` identifies the batched owner guard;
  `filter_join_seeded_driver` and `filter_join_seeded_output` identify seeded
  execution. Their `input_rows` are the actual driving relation's cardinality,
  which can be much greater than the final answer count. BGP and filter-selector
  path inputs are one initial seeded relation, not the number of variable bindings
  in its seed. Their output count is the actual selected candidate relation.
- `choices` retains at most 64 individual dispatch observations in completion
  order. `choices_dropped` and `choices_complete` explicitly identify sample
  saturation. Aggregate path counts remain available after saturation.
- A missing output count is unknown, not zero. `calls_with_output` identifies
  how many dispatches contributed to an aggregate `output_rows_sum`.

Inclusive times overlap and include instrumentation overhead. Do not sum them
as request wall time. The scope is local `tool_query`, including any inference
and label reads. It excludes HTTP admission, acquiring the store reader, and
network transit. The coverage field explicitly limits interpretation: this is
not SQL `EXPLAIN`, SQLite VM-step accounting, or every optimizer decision.
A missing event cannot prove an optimization was never considered.

The profile contains no query text, RDF values, graph names or caller labels.
State is isolated to the calling execution thread, bounded, and restored on
success, error and panic. Unsupported federated or standard SPARQL protocol /
content-negotiated responses refuse profiling instead of silently discarding it.
It is not a persistent server setting and does not authorize replay workloads.

Tests compare the real query-tool answers with profiling on/off over both sparse
and dense named-graph guard fixtures. The HTTP route test checks opt-in response
shape and unsupported transports. Additional controls cover saturation, explicit
loss, unknown outputs, nested scope isolation and restoration after failures.

Deployment and runtime attribution are separate: after the normal source lane
lands and the served revision is verified, a bounded authorized replay of the
same request can show actual selected paths. These source controls make no
claim about a production query's dominant cost or recovered latency. Rollback
is an ordinary revert of the diagnostic source change through the same lane.

## Local demonstration

The [terminal recording](output.log) and [timing file](timing.log) show the same
positive one-row query with profiling off, then on, over a local in-memory store.
The normal answer stays identical. Watch the filter report two candidates and
one survivor. This is a source candidate demonstration, not an installed service
or a production latency comparison. The base is `9a23c3b63e11a7879c6aa72ccbe5d70a7636ad81`;
the base runtime was not rebuilt for this recording. Exact demonstrated source
file hashes are below; the PR identifies the candidate commit.

Reproduce with `cargo run --no-default-features --example execution_profile`.
For terminal playback, download both files and run
`scriptreplay --log-out output.log --log-timing timing.log`. Playback was checked
locally; no inline-player compatibility is claimed. These small sanitized
recording files are retained beside this existing repository demo directory.

- `src/sparql/profile.rs`: `d77b60abf84e989ea8c6baeac0123e0fc00652e45bc0c2207e3466a7ee530a58`
- `src/sparql/pattern.rs`: `04009fcbcb97b1fe24f497b16d6716228b50bee5c3bae075eae567822931a912`
- `examples/execution_profile.rs`: `bc45e9f34242312ebd12f852eb4ce8d25d6f96426ef141c29a8783c6a7741fcf`
