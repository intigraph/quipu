# Recorded verdict witness compatibility

The same canonical-only signature test fails with the legacy-only reader and
passes with the compatibility reader. The candidate also passes ambiguity,
revocation and registration-authority negatives: 14 trust-root tests in total.

Replay the [terminal recording](output.log) with its [timing](timing.log):

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The recording contains actual native test output. Its command header omits the
private packet path. It was captured before the test-only borrow lint correction;
all production reader bytes are unchanged, and all 14 tests passed again after
that correction. The original run failed the later quality gate on that lint;
the separate corrected run passed the full gate. This is source verification,
not installed or runtime acceptance.

On the candidate, reproduce the affected suite with:

```sh
cargo test --locked --features full --lib verdict_trust_root_tests -- --test-threads=2
```
