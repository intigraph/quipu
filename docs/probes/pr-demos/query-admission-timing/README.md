# Query admission observation demo

The same in-memory HTTP query returns one real row with the option disabled and
enabled. Disabled observations omit `admission_ms`; enabled observations start
with null and end with a granted-acquisition value no greater than `dispatch_ms`.
Only allowlisted timing fields are printed. No server listens on a port.

Reproduce from the repository root:

```sh
python3 docs/probes/pr-demos/query-admission-timing/demo.py
```

The output/timing pair is a terminal recording, not a video or inline player:

```sh
scriptreplay --log-out docs/probes/pr-demos/query-admission-timing/output.log \
  --log-timing docs/probes/pr-demos/query-admission-timing/timing.log
```

The recording demonstrates the new configuration on a candidate fixture. The
source base is `dd007300dc35c922aa4da8e4b3c710ecd3829cf0`, whose observation
records have no admission field. It does not demonstrate activation, production
latency improvement, or attribution of a production tail. The default stays off.
The implementation revision is `88070f0a`; the recording-only follow-up leaves
its Rust sources unchanged, pinned in [the source manifest](source-sha256.json).

See [the contract](../../../design/query-admission-observation.md) for null,
cancellation, timer-origin, and rollback semantics.
