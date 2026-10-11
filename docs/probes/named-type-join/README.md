# Named-graph type join probe

Constant type patterns in a single named graph use asserted facts. Their BGP
planner nevertheless disabled the read-model hash join as though ROOT subclass
inference were required. The planner now uses the same inference scope as the
leaf evaluator. ROOT inference, temporal reads, attachments and the model's
affordability ceiling retain their existing guards.

## Before and after

The [actual transcript](output.log) uses a disposable synthetic database with
75,000 facts, 25,000 subjects, 14,283 subjects assigned to one owner and 100
eligible results. The query contains owner/type/status branches, missing and
unknown status handling, ordering and a result limit. No production data was
copied. It exercises four concurrent readers in three barrier-synchronized
rounds with the same ten-second deadline.

The SQL control's slowest sample is 3.145226 seconds; the model candidate's
slowest is 0.614951 seconds. All ordered results agree exactly. Neither private
mode timed out. This is a private query-cost demonstration, not proof that a
served timeout or its workload has recovered. The transcript is readable text;
no hosted playback is claimed.

The separate mechanistic regression asserts that a named type join actually
builds its graph model. Returning correct rows through SQL alone cannot pass
that control. Existing ROOT subclass controls remain required.

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
Normal unit tests do not run the external probe. Each reader starts without a
resident model; this includes the cold model cost.

Rollback is a normal reviewed revert of the planner change. Runtime acceptance
must measure the actual caller query under its matched workload and verify its
error rate separately. Increasing the timeout is outside this change.
