# Abandon a reserved model handoff

A source reserves a handoff to B, then abandons that exact attempt. Watch the
recorded head change from `RESERVED` to `ACTIVE` at the same source tuple, with
an immutable abandonment outcome. B retries every old handoff step: each refuses,
B remains unable to commit, and the source remains writable.

The [terminal recording](output.log) and [timing file](timing.log) contain actual
JSON observations and assertions from an isolated, I/O-free Rust model test.
They are small retained repository artifacts. This models protocol behavior;
it does not demonstrate installed storage fencing, authenticated witness access,
or a production ownership transition.

Base: `5db25139fecbe3a302a31cdc62fc598636d62096`. Model source:
`d127b447426726a86d3ca76d8024557c9dfc58a3`. The base has no `Abandon` action;
the before/after states are therefore both observed inside the candidate
fixture, rather than claiming a rebuilt base supports the same call. The copied
test executable was built from these exact model source bytes before recording.
Later recording/caption additions preserve them.

Reproduce from the repository root:

```sh
just test --features full --lib ownership_handoff_model::tests::abandoned_attempt_cannot_activate_its_stale_destination -- --exact --nocapture
```

Download both recording files and play them with util-linux `scriptreplay`:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Playback is checked locally; no inline browser player is claimed. To record a
new run, copy the compiled test executable into an owned directory, set
`OWNERSHIP_MODEL_TEST_BINARY` to that copy, and invoke [run-demo.sh](run-demo.sh)
inside a fresh util-linux `script` terminal. It changes only that terminal's
newline conversion. No service, credentials or network are required.
