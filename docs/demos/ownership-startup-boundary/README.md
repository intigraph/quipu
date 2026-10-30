# Enrolled startup: refuse before migration

The [recording](output.log.gz), [timing](timing.log) and
[transcript](transcript.txt) run the same disposable SQLite library probe on
base `ad2cc8878f1e52439fb04ede96eb8a8d684ecdfc` and candidate
`8a836e4ffac1cd45f0aa7e826c8ab3a3640e1476`.

Watch `writable_startup` and `explicit_adoption`: both change from ADMITTED to
REFUSED when the fixture contains the reserved handoff-enrollment marker.
Ordinary unenrolled startup stays ADMITTED; read-only inspection stays AVAILABLE.
The candidate leaves the marked database schema unchanged.

The marker is a private fixture input, not an authenticated enrollment record.
No witness, journal adapter, signer, authority profile, production database or
native handoff participates. This demonstrates the startup refusal boundary;
it does not demonstrate enrolled admission or physical writer exclusivity.
The [source contract](../../design/ownership-startup-boundary.md) lists the
remaining gates.

Reproduce the candidate with cached dependencies:

```sh
just demo ownership-startup-boundary
```

For the comparison, build `examples/ownership_startup_boundary.rs` against base
and candidate source in isolated checkouts, then pass the two executables to
[`probe.sh`](probe.sh) as `STARTUP_BASE_PROBE` and `STARTUP_HEAD_PROBE`. The script
checks actual output from each invocation. Crew qualification uses the normal
HOME guard; full compiler and guard logs stay private.

These small sanitized recording artifacts follow the repository's existing
`docs/demos` retention. Playback requires downloading both files:

```sh
gzip -dk output.log.gz
scriptreplay --log-out output.log --log-timing timing.log
```

Use `--divisor 0.05` to slow this short fixture run.

The transcript normalizes CRLF line endings for text tools. The compressed
recording and timing retain their original bytes.
