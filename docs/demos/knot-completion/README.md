# Ordinary write completion receipts

The same private HTTP write succeeds on both binaries. Watch its completion
lookup change from **404** to an exact `finished` transaction receipt, while
reusing its ID changes from a second accepted dispatch to a refusal. A
never-seen ID returns **unknown**, never a not-started proof.

- Before: copied server `44a403f51645585bc6e5ae9c0cf82abda85bc2e9`, clean.
- After: source `c3ba3bef9296c958fa0bb3d1060cd4c6ae3e7284`, clean, full bundle.
- The following commit adds this recording and reproducer; server source is
  unchanged from that demonstrated implementation.
- Targets: disposable local SQLite stores and loopback HTTP servers. No
  installed service, account, credential, schedule or runtime journal changes.

[Actual terminal output](output.log) and [timing](timing.log) are small retained
repository artifacts. Download both and replay with:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Playback was checked with `scriptreplay`. To repeat with your own corresponding
binaries:

```sh
python3 docs/demos/knot-completion/demo.py BEFORE_SERVER AFTER_SERVER
```

The recording exercises real HTTP writes and receipt lookups, not a simulated
server response. It does not inject a live client timeout. The separate native
regression `knot_completion_late_commit_outlives_cancelled_http_future` holds the
writer, expires the client wait, cancels the HTTP future, confirms `running`,
then releases the original worker and observes its transaction without replay.
Other controls cover bounded eviction/age, foreign identities, wrong request
bodies, real bearer/read-only middleware and expired/disabled deadlines.

Process-local receipts are not durable idempotency. Missing, expired, evicted or
restarted-server receipts remain UNKNOWN. Production installation and the
consumer's complete effect readback remain separate acceptance steps.
