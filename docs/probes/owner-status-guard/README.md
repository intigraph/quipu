# Owner status guard: selective subject binding

A grouped status guard followed by owner/type triples used to evaluate the guard
for unrelated subjects before applying the owner join. The candidate evaluates
the existing subject relation first, then runs the unchanged guard with those
bindings. It admits only filtered BGPs whose driving BGP adds no variables;
volatile expressions and scope-bearing operators keep the original evaluator.

The [terminal recording](output.log) and [timing file](timing.log) demonstrate
production-code commit `81948611`, based on `4d1dcfe7`. Both paths run in one
binary against the same synthetic store: 12,000 Actions in two named graphs,
with four owned subjects per graph. The baseline explicitly disables only this
optimization through a test-local switch, retaining the original independent
join. This is a local library demonstration, not a deployed service measurement.

Observed execution times, excluding fixture construction:

| Path | First run | Second run |
| --- | ---: | ---: |
| Original independent join | 252.177 ms | 229.410 ms |
| Subject-bound guard | 5.363 ms | 6.096 ms |

The all-valid guard returns false on both paths. Adding an invalid status to an
owned Action returns true on both paths. Run order is baseline, candidate,
candidate, baseline. These few samples establish the effect on this fixture;
they do not establish a production latency distribution.

Reproduce with cached Cargo dependencies:

```sh
bash docs/probes/owner-status-guard/run-demo.sh
```

Replay after downloading both recording files:

```sh
scriptreplay --log-out output.log --log-timing timing.log --divisor 10
```

This is an output-only util-linux terminal recording, not a video or inline
player. Playback was checked. The small recording and caption are retained here
with the source for review; no external artifact host is required.

Differential tests also compare complete bags and headers with the original
evaluator, including status types, owner encodings, ROOT/foreign-graph exclusion,
and a read-only provider supplying status facts in the same named graphs.
