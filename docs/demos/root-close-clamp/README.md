# Preserve ROOT history when an incoming close equals its start

The new common-core regression supplies a ROOT value valid from
`2026-01-02T00:00:00Z` and an incoming removal at that same instant.
The baseline closes ROOT at its own start, making the interval empty.
The fix clamps this close to decision time `2026-01-10T00:00:00Z`, preserving
its earlier history as required by oracle R5.

The fixture repeats the same instant as `2026-01-01T19:00:00-05:00` and covers
both single-valued and unbounded set slots. Existing later-close controls remain
in the merge suite. This is a synthetic library plan, not installed acceptance.

Baseline: `396e7881d80e980d2686cee55249c050c61495b2` with the new regression.
[Actual baseline failure excerpt](baseline.txt) is a transcript, not a recording.
The candidate recording prints actual planned closes for all four combinations.
Its exact source revision is recorded in the log header.

```sh
just test --locked --no-default-features --features shacl,owl --lib an_incoming_close_equal_to_root_start_must_preserve_root_history -- --nocapture
```

Download [output](session.log) and [timing](session.time), then replay locally:

```sh
scriptreplay --log-out session.log --log-timing session.time
```

The pair is retained with this source; playback requires util-linux scriptreplay.
