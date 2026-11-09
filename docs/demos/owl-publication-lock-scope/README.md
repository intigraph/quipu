# OWL publication lock-scope controls

This terminal recording runs two synthetic controls through the actual materializer
and Store write path. Pausing private preparation and acknowledgement permits a
concurrent write to complete. A negative control holds the live mutex and confirms
the same write waits until release. Both controls pass.

The implementation is `03f341fc`; integration head
`717ddb1c5f514ec60e37340268dc72e2c7d4ecd3` has identical server, OWL publication,
and timing source. Timers were committed first at `0176c3ed`. The retained full
server test binary has SHA256
`cb3f649f30fa3e23f859c4e02e29f7f3c4ae26160957735c18136f272a829509`.

Download [output](output.txt) and [timing](timing.txt), then replay with util-linux:

```sh
scriptreplay --log-out output.txt --log-timing timing.txt
```

The output also serves as a readable transcript. Reproduce from the source checkout:

```sh
just test --features full --bin quipu-server owl_materialize::publish::test_gate -- --nocapture --test-threads=1
```

This compares deliberately held and released lock scope in local fixtures; it is
not an old-revision benchmark or production-size latency measurement. Production
acceptance remains unmet. The recording contains synthetic identifiers and bounded
stage metadata, with no payloads or credentials. These small recording files are
retained here alongside the implementation; playback requires a terminal replay
utility rather than an inline video player.
