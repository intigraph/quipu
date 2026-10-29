# Durable outcome replay demonstration

This is an offline source demonstration with synthetic journal records. It does
not exercise a running search server or establish installed capture coverage.

The baseline `fd93c2b0b3c2eb77e2375935bedf7ac49c3106e0` lacks the replay command.
The candidate consumes one successful empty query, one failed query and an
identical duplicate. Watch the separate `empty` and `error` series both remain
one, then stay unchanged after reopening state and replaying the same journal.

Reproduce from the repository root:

```sh
python3 -B docs/demos/search-outcomes/probe.py
```

The small byte-exact output-only recording is gzip-compressed; its timing file
is retained with the source as review evidence. Download both to replay with
util-linux:

```sh
gzip -dc output.log.gz > /tmp/search-outcomes-output.log
scriptreplay --log-out /tmp/search-outcomes-output.log --log-timing timing.log
```

[Recording](output.log.gz) · [Transcript, normalized line endings](transcript.txt)
· [Timing](timing.log). Playback requires downloading and
a terminal replay tool; no inline player is asserted. The PR caption identifies
the demonstrated candidate commit. No private queries, credentials or service
addresses are used in this fixture. These artifacts follow normal repository
history retention; no separate media store is provisioned.
