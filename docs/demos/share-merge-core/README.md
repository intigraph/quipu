# Share merge core demonstration

This isolated library fixture reconnects a share and explicitly selects the
incoming value. On baseline `02268bed5ea4f3a90eaef9551a65816698e87503`,
the same new regression saw one live assertion and no received receipt.
On source `b067b99075167731a3ff88cb82108cc587e0a964`, it checks two ordered
assertions: the received value has an empty valid interval at decision time,
and the chosen live assertion starts at that same time in a later transaction.

[Actual baseline failure excerpt](baseline.txt) is a readable transcript,
not a baseline recording. The candidate recording exercises the same regression
plus the independent-writer and late-validation rollback controls. These are
synthetic local stores, not an installed-service or production acceptance claim.
The demo-only follow-up changes no Rust source.

Reproduce from the source checkout:

```sh
just test --locked --no-default-features --features shacl,owl --lib decided_share_records_receipt_then_live_winner_at_decision_time
just test --locked --no-default-features --features shacl,owl --lib decide_gate_failure_rolls_back_the_received_step_and_clean_changes
just test --locked --no-default-features --features shacl,owl --lib reservation_excludes_an_independent_writer_and_releases_after_failure
```

Download [candidate output](session.log) and [timing](session.time), then replay:

```sh
scriptreplay --log-out session.log --log-timing session.time
```

The output/timing pair is retained with this source. Playback requires downloading
both files and util-linux `scriptreplay`; no inline video playback is claimed.
