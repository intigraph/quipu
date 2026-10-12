# Selective named-graph query probe

Owner/type/status branches scanned many closed subjects before finding rare
active results. The final planner uses a complete small literal relation first:
probe up to 129 rows, use SQL for at most 128, otherwise retain normal planning.
The plain-property absence optimization now recognizes fixed same-subject
owner/type constraints and still evaluates the original filter on survivors.

ROOT inference, historical reads and resident-model budgets retain
their guards. Composed archives use the bounded paths described below. The broader named-type hash-join experiment was not retained:
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

## Composed archive control

The server synchronizes frozen archive attachments on pooled-reader acquisition.
Opening a read-only Store alone omits that step. The external probe now invokes
that synchronization and reports the mounted count. A fixture with an unrelated
archive must report one attachment before its timings describe composition.
`QUIPU_PERF_EXPECTED_ATTACHMENTS` verifies the declared mounted count.
Optional `QUIPU_PERF_EXPECTED_SUBJECTS` supplies independently derived, ordered
subject IRIs so a candidate cannot silently establish its own incomplete oracle.

An unrelated archive previously disabled the selective paths, making each typed
owner star repeat composed SQL for thousands of subjects. Current single named
graphs can now hash-join complete SQL relations capped at 50,000 rows; a 50,001st
row restores the existing planner. Small literal probes count canonical bindings
across composition. Unbound triple scans use a complete, statement-local alias
snapshot up to 4,096 entries; a larger alias table retains indexed lookup.
Absence candidates compose all layers and recheck every original BGP constraint
and filter. These bounds cap candidate or binding materialization, not total database work;
SQLite grouping can still precede a row stop. Deadlines remain enforced.
No archive is detached, and no query budget is raised.

The [mounted before transcript](composed-before.log) records 24 deadline errors
in a synthetic fixture with 100 eligible subjects. The
[mounted after transcript](composed-after.log) records the same four-reader,
three-round workload in both model modes, with no timeout and a slowest read of
1.050953 seconds. These are private experiments; served recovery still requires
matching the actual caller and deployment version independently.

Composition controls also reject an omitted mount rather than timing the wrong
context, and retain selective lookup when a broader relation encounters malformed
unrelated data. Dense class candidate probes and small intermediate-row caps
keep the existing evaluator instead of building an oversized alternative plan.
