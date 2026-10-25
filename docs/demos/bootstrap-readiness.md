# A bootstrap gap cannot make a follower ready

This is a retained **text transcript**, not a recording. The original failing
run was not recorded; its actual output is preserved below rather than recreated.
Both runs use a private in-memory six-transaction source and the real shared
applier. No service or operational follower is used.

Base: `81ed893708ebc407dcec75f0c877db95274015e1`, with the regression test added.
Repair: `eae0f534`, integrated with current main in `1b1669d5`.

The source supplies transaction 1, then a page starting after transaction 3.
The applier keeps its durable cursor at 1. Before the repair, bootstrap still
returns success. After the repair, bootstrap returns a retryable transport error
and readiness stays false. The same test also checks a nonadvancing page and a
normal retry from cursor 1 through transaction 6, which becomes ready.

Repeat on the candidate:

```sh
cargo test --locked --lib bootstrap_gap_or_nonprogress -- --nocapture
```

Actual base output:

```text
running 1 test
thread 'follow::tests::bootstrap_gap_or_nonprogress_cannot_mark_a_partial_follower_ready' panicked:
gap=true: Ok(2)
test follow::tests::bootstrap_gap_or_nonprogress_cannot_mark_a_partial_follower_ready ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3010 filtered out
command exit: 101
```

Actual repaired output:

```text
running 1 test
test follow::tests::bootstrap_gap_or_nonprogress_cannot_mark_a_partial_follower_ready ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3010 filtered out
command exit: 0
```

Both focused runs had clean HOME guards (157 paths and five UID resources).
The transcript omits compiler warnings, process IDs and elapsed times. This
shows candidate library behavior; it does not prove production failover routing.
