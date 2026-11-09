# Bound incoming targets use the value-and-attribute index

This is a retained **text transcript**, not a recording. The original failing
run was not recorded; its actual output is preserved below. The reproducer uses
the real helper and an isolated in-memory store, with 10,000 current facts that
share the requested value and graph but have a different predicate.

Before: `a3a94905d8d9958683967faaaa6c61d7ece3d15d`, with the regression test added.
After: `94a188f886b2982794188bc7ceffd4e185023d25`; current-main integration
preserves the same lookup and tests. The old plan uses `idx_active_vge`, whose
prefix is value plus graph. The repair explicitly selects existing `idx_vaet`,
whose prefix includes the bound attribute. Graph, current-assertion filters,
returned fact fields and the 100,000-fact budget remain enforced.

Repeat on the candidate:

```sh
cargo test --locked --features shacl --lib store::shacl_poststate::lookup_tests:: -- --nocapture
```

The work regression interrupts after eleven callbacks at 1,000 VM instructions
per callback. The old helper exhausts that deterministic work bound; the repaired
helper returns its one matching fact. This is a work-limit demonstration, not a
wall-clock benchmark of the synthetic fixture.

Actual before output (failure location omitted):

```text
called `Result::unwrap()` on an `Err` value: Sqlite(SqliteFailure(Error { code: OperationInterrupted, extended_code: 9 }, Some("interrupted")))
test store::shacl_poststate::lookup_tests::incoming_lookup_bounds_work_for_unrelated_attribute_cohort_and_keeps_cap ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3706 filtered out; finished in 0.09s
```

Actual after output:

```text
running 2 tests
test store::shacl_poststate::lookup_tests::incoming_lookup_preserves_complete_row_identity_across_values_graphs_and_history ... ok
test store::shacl_poststate::lookup_tests::incoming_lookup_bounds_work_for_unrelated_attribute_cohort_and_keeps_cap ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 3705 filtered out; finished in 0.11s
```

The identity control compares complete returned facts with both known expected
facts and the old value-and-graph index plan. It returns rows for reference,
string, integer, typed and language-tagged values, in ROOT and named graphs,
across two predicates. Retracted/history rows, another graph and an unknown
predicate are excluded. The same helper also refuses a full fact budget and
accepts its last available slot. Existing cardinality and rollback controls
passed separately.

One isolated 43-request CLI write replay completed in **7.176955 seconds** on the
after revision, versus **32.066810 seconds** on the before revision: **4.47 times
faster**, with the under-five-second target still **unmet**. Both used the same
pristine corpus, client and SDK, feature set, unoptimized/debug-zero build
profile and per-copy cold-cache policy. All 43 responses completed with HTTP 200;
request and response body totals and CLI output matched. Expected per-run times
and generated write identifiers differed. This is source-candidate evidence,
not installed-service acceptance or a benchmark of the later integration head.
No repeat of the full replay was performed.

The first incoming-target phase fell from 12,544.860 ms to 3.008 ms, and the
shared value-context phase from 10,473.702 ms to 1.885 ms. The existing index also
contains historical rows and has no graph-leading prefix: this deliberately
fixes the demonstrated broad value cohort, and does not promise a universal win
for every history or graph distribution. No schema or new index is introduced.

Retention: this small, sanitized transcript stays in the repository. Private
corpus contents and operational request/response logs are not published.
