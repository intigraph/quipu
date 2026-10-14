# Failed acquisitions do not admit history

Trigger: refuse an initial one-byte acquisition, advance MAIN from transaction2
to3, then obtain the first successful receipt and try its generation at pin2.

On source436e99c3314c9ffec66ff846cfdf37920268094d, the failed request seeded
floor2. The first success retained that floor and accepted pin2.
[Actual baseline transcript](anchor-before.txt) comes from the new regression
fixture run against that source; it is not a recording.

The candidate at e5e19f5a9e37fc117549068cea7836c85f865ca5 publishes the anchor
after final validation and read commit. The same fixture reports floor3 and
refuses pin2, while the observed current pin remains valid. It uses a private
SQLite store, not a deployed service.

[Candidate recording](anchor-output.txt) and [timing](anchor-timing.txt) are
retained as small public artifacts alongside the original composed demo.
Download both to replay:

```sh
scriptreplay --log-timing anchor-timing.txt --log-out anchor-output.txt
```

Repeat the exact control:

```sh
cargo test --lib failed_first_acquisition_cannot_admit_an_earlier_history_floor -- --nocapture
```

Additional controls verify that17 refused new scopes consume no history slots
and that a failed generation change preserves the previously admitted anchor.
The existing observed-history, ABA, alias and lease controls remain required.
No installed behavior, SDK acceptance or inline playback is claimed.
