# Release readiness: pending is not failure

The same queued, running, failing and successful check fixtures are supplied to
the actual waiter's `main()` on base and candidate, with a one-poll budget.
Queued/running changes from exit 1 to exit 0 with `ready=false`; a genuine failure
still exits 1; only success emits `ready=true`. The workflow uses that explicit
output to keep publication skipped while deferred.

- Base: `730824e85297f7824c99ccdd0f36ecb0852f1962`.
- Demonstrated implementation: `bfed5997e39a0260acc82d8ad302b6d913edf042`.
- Target: source/library fixtures with the check API mocked; no live job rerun,
  publication or deployment is demonstrated.
- Recording: [terminal output](terminal.log) and [timing](terminal.time).
  These are a replayable terminal recording and readable transcript, not a video.

Replay after downloading both files (util-linux `scriptreplay`):

```sh
scriptreplay --log-out terminal.log --log-timing terminal.time
```

Repeat from the repository root:

```sh
python3 docs/demos/release-readiness/demo.py \
  730824e85297f7824c99ccdd0f36ecb0852f1962 \
  bfed5997e39a0260acc82d8ad302b6d913edf042
```

The small sanitized recording is retained with its reproducer in the existing
`docs/demos` directory. Follow-up recording/caption commits do not change the
demonstrated waiter source. Playback was checked with `scriptreplay`; inline
playback in a forge page is not claimed. Completion-order/retry safety is covered
by the separate rearm and workflow-contract tests, not this recording.
