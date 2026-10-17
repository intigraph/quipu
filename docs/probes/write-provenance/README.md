# Durable declared write provenance

This [recording](output.txt) ([timing](timing.txt)) shows the library against an
isolated legacy-schema fixture, then the same fixture after a writable open and
one request-scoped commit. Before, provenance is unavailable. After, the exact
transaction retains agent, harness, host, session, model, task and client version.
The idempotent-write and unknown-session controls produce no declaration rows.

The pre-change source is `abb9e5a1aa6b44dd43c9652a6cd0747e54a4fa2e`;
the candidate is the source beside this example. The demonstration uses a
synthetic legacy fixture because the base has no durable provenance read API.
It does not write to a serving endpoint or establish production migration,
all-client coverage, Session RDF projection or historical reconstruction.

Build and run with `just --justfile docs/probes/write-provenance/demo.just demo`.
Replay with
`scriptreplay --log-out docs/probes/write-provenance/output.txt --log-timing docs/probes/write-provenance/timing.txt`.
Playback requires a terminal and util-linux `scriptreplay`; no inline video is
claimed. The small sanitized recording is retained beside this reproducer.
