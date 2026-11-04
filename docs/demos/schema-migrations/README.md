# Schema migration budget demonstration

Four identical template facts have one net assertion. The baseline planner
accepted them under a materialization ceiling of three. The candidate refuses
before retaining the fourth fact and accepts at the exact ceiling of four.
The filter control proves an empty final answer cannot hide ten intermediate
rows; the configured native row ceiling can still impose a stricter limit.

Baseline product code: `e318b0f9e65297273ba6fe92062d7eaf4b0945c0`, with the
new duplicate-materialization regression added. `before.txt` contains authentic
output from that retained failing run. It is historical evidence; the recording
does not rerun the baseline executable. Candidate product code:
`73395a202d41c3fab9c1811860fae00402b7de5e` (base `8b8bf827`).
Later changes adding these demo artifacts do not alter product code.

The terminal recording combines the retained baseline excerpt and actual
candidate test invocations. These are isolated source fixtures, not an installed
service or production benchmark. No runtime migration is executed.

Reproduce the candidate controls from the repository root:

```sh
just test --locked --lib --features shacl repeated_template_facts_are_bounded_before_netting
just test --locked --lib --features shacl filtered_where_intermediates_share_the_migration_ceiling
```

For the exact recording script, set `MIGRATION_TEST_BIN` to the compiled SHACL
library test executable, enter this directory and run `bash run.sh`.
Download `output.log.gz` and `timing.log`; decompress the output, then play:

```sh
gunzip -k output.log.gz
scriptreplay --log-out output.log --log-timing timing.log
```

`transcript.txt` is the readable fallback. Small sanitized artifacts are retained
in this repository with the PR; no external recording host is required.
