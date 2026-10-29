# Explicit store ownership

An explicitly ENFORCED database copied to a new path refuses writes until
explicitly adopted there. Stage A legacy databases remain INACTIVE; that
compatibility state is not ownership acceptance.
This local candidate demo reports initial INACTIVE status, explicitly adopts
the source, makes a SQLite backup, observes the copied-store write
refusal, adopts it and repeats the same write successfully. A second adoption
must produce another epoch. All files are disposable; no service or installed
store is touched.

```sh
QUIPU_DEMO_BIN=/absolute/path/to/qualified/quipu python3 docs/demos/store-ownership/run-demo.py
```

The disk/source/applier tests additionally check signed `epoch_end` and refusal
to splice an old follower into the new epoch. Paired gate-disabled mutants are
test evidence, not a deployed baseline. Production activation requires separate
operator admission, including adoption of legacy stores and follower bootstrap.

## Recorded candidate

[Terminal output](output.log) and [timing](timing.log) form a replayable terminal
recording, not a video. Replay from this directory:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The recording shows candidate behavior on an isolated fixture: the copied path
refuses the write, then the same write succeeds after explicit adoption. The
second adoption prints a different epoch. Automatic embedding is unavailable
in this deliberately minimal fixture; the successful write proves graph
mutation, not semantic-index readiness.

Base source is `8b4e3835`. The executable was built from the candidate runtime
source recorded in [the SHA-256 manifest](source-manifest.json), before its
commit; the frozen source commit is identified in the PR. This is not an
installed or release-artifact acceptance claim. The recorded terminal contains
real responses, including the expected refusal.
