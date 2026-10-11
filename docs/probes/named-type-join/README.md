# Selective named-graph query probe

Owner/type/status branches scanned many closed subjects before finding rare
active results. The final planner uses a complete small literal relation first:
probe up to 129 rows, use SQL for at most 128, otherwise retain normal planning.
The plain-property absence optimization now recognizes fixed same-subject
owner/type constraints and still evaluates the original filter on survivors.

ROOT inference, historical reads, attachments and resident-model budgets retain
their guards. The broader named-type hash-join experiment was not retained:
cold graph construction costs too much for this workload.

## Before and after

The original [small-fixture transcript](output.log) and
[near-budget transcript](near-budget.log) record the initial experiment. The
larger fixture contains 958,355 current facts, 25,000 subjects, 14,283 subjects
assigned to one owner and 100 eligible results. Padding changes only unqueried
attributes. No production data was copied.

Four-reader barrier rounds use the same ten-second deadline and compare every
ordered result. Original near-budget SQL's slowest sample is 4.080455 seconds.
The final [selective transcript](selective.log) records the completed narrower
implementation. Neither private mode times out. These are private cost
measurements, not proof of served recovery; the actual caller and workload must
be measured independently after reviewed adoption.

The sparse relation test proves the planner avoids whole-graph construction.
The 129-row control proves a truncated probe never replaces the full query
answer. Differential absence controls retain plain, typed, language, numeric,
IRI and missing properties, fixed owners, duplicate type patterns, graph
isolation, and exact fallback for unsupported shapes.

## Reproduction

Use an explicitly disposable database and matching query file:

~~~sh
QUIPU_PERF_DB=/path/to/disposable.db \
QUIPU_PERF_QUERY=/path/to/query.sparql \
just test --release --no-default-features --lib \
  external_query_fixture_parallel_perf -- --ignored --nocapture
~~~

The ignored probe opens its database read-only, compares exact ordered results,
reports actual timeout errors separately, and refuses an empty positive control.
Normal unit tests do not run the external probe. The transcript is readable text;
no hosted playback is claimed.

Rollback is a normal reviewed revert. Acceptance must measure the actual caller
under its matched workload and verify the deadline failure rate separately.
Increasing the timeout is outside this change.
