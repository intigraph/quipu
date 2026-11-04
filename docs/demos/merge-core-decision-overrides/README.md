# Merge decision overrides

The same synthetic slot has base `{a}`, ours `{c}`, incoming `{c}` and
`maxCount=1`. Watch the `Base`, explicit two-value and `Continue` results:
before, `Base` is refused and the oversized selection is accepted; after,
`Base` restores `{a}` at decision time and the oversized selection is refused.
The `Continue` control produces no operations in both runs.

This records source-library test binaries, not an installed service. The
before core is revision `9da19a6e`; the after core is `9d3e27f4`. Both binaries
include the identical `decision_override_demo` test from this change. Subsequent
recording-only changes do not alter the demonstrated core.

[Terminal recording](session.log) and [timing](session.time) are retained here
as small review artifacts. Download both and replay with util-linux:

```sh
scriptreplay --timing session.time --typescript session.log
```

The recording is also a readable transcript; no browser embed is required.
Reproduce the after result from this PR checkout with:

```sh
just test --locked --no-default-features --lib \
  merge_core::tests::decision_override_demo -- --exact --nocapture
```

To reproduce the before result, copy this same test onto the before revision
in an isolated checkout. Expected test exit: 101 before, 0 after.
