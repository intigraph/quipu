# Reference aggregate DISTINCT

A release-profile direct aggregate control over 30,714 unique reference bindings
returned the same count before and after the change. Isolated evaluation took
1,393.633 ms before and 3.127 ms after in one bounded paired observation. Small
unique, duplicate and unbound controls also returned the same counts.

Reproduce each fixed case independently:

```sh
cargo run --release --example distinct_reference_cost -- positive
cargo run --release --example distinct_reference_cost -- duplicates
cargo run --release --example distinct_reference_cost -- unbound
cargo run --release --example distinct_reference_cost -- distinct_refs
```

The control calls `eval_aggregate` directly with an in-memory store. It excludes
query parsing, graph acquisition and the streaming COUNT planner. These timings
do not establish whole-query latency or historical-data performance. Each measured
process had a five-second wall cap, 512 MiB memory limit, no swap and one CPU.
The candidate timing guard detected a concurrent change to the installed CLI;
that guard result remains failed with cause unknown. Both measured executables
were frozen private copies; the timing batch was not repeated.

Paired semantic controls preserve mixed reference/literal equality, first SAMPLE,
distinct and ordinary counts, empty groups and the existing refusal to MIN/MAX
unresolved reference IDs. Those paired controls passed with a clean HOME guard.
