# Reader startup degradation

Trigger: a foreign listener holds the configured reader port. Before, the
primary exits with `Address already in use`. After, authorized primary writes
still succeed, health and data reads return explicit 503 responses, and the
foreign listener receives no connection. A free-port control still serves reads
through a distinct reader process.

This is an isolated temporary SQLite fixture, with synthetic authorization.
The baseline binary is clean `1792a7a6771583876522cff6fa0dfa0f31e7b6ce`;
the candidate is clean `5ed16bbf1f39b9a61e2c3354a9d9c95a75c6fe67`.
The candidate used a development build with `server,onnx` and
`RUSTFLAGS="-C debuginfo=0"`. This is a behavior demonstration, not a
performance or production acceptance measurement.

Reproduce with separately built binaries:

```sh
python3 docs/demos/read-process-startup/demo.py BASE_SERVER HEAD_SERVER CLI
```

[Recording](demo.cast) is an asciinema v2 terminal recording converted from
actual output/timing files, with output bytes and timing order preserved.
[Transcript](transcript.txt) is the readable fallback, with terminal CRLF
normalized. Download the recording and use `asciinema play demo.cast`; no hosted
inline player is provided. The original output/timing replay was checked with
`scriptreplay`; the converted recording was checked for exact output-byte
round-trip. Only the baseline error line is extracted from the expected
failure's detailed fixture log. These small artifacts are retained with the
source demonstration.
