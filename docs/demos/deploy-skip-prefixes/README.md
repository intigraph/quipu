# Standing prefix proof demonstration

Base `ce86ba4cc1b5bf0bcf98a67c2de8a6e1f075946b` has no standing prefix CI
job. This source candidate adds one. `just deploy-skip demo` shows a safe isolated
fixture passing, then rejects a source include and a later data-only change to
its input. No deployment consumer is changed or activated by this demonstration.

The output and timing files are a util-linux terminal recording, retained here
with the reproducer. Download both and run
`scriptreplay --log-out output.log --log-timing timing.log`; the output is also a
readable transcript. This records source behavior, not installed acceptance.
