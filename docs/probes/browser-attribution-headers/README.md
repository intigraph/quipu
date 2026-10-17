# Browser attribution preflight

A browser requests POST `/query` with `Content-Type`, `X-Quipu-Client` and
`X-Quipu-Task`. The old allowlist advertises only `content-type,authorization`,
so the browser blocks the request. The candidate additionally advertises the two
attribution headers. An explicitly configured origin allowlist still rejects a
sibling origin; arbitrary headers and credentialed CORS remain disallowed.

Run `python3 docs/probes/browser-attribution-headers/run-demo.py` in an isolated
checkout. It executes real in-process HTTP OPTIONS requests through the server's
CorsLayer. Its BEFORE control removes exactly the two new names, reproducing the
old allowlist, then restores the file and repeats the same probes. This is a
synthetic HTTP fixture, not an installed service or a full base binary. Do not
run it alongside source edits. The script restores the source on failure.

`output.log` and `timing.log` are a small terminal recording retained here with
the source. Replay with `scriptreplay --log-out output.log --log-timing timing.log`;
the output file is also the readable transcript. No inline player is assumed.
The configured-origin negative control passes in both phases; the attribution
preflight fails before and passes after. Build warnings and local paths are
excluded from the displayed output; no credentials or network calls are used.

Base revision: `05c916c8c385101f7a9210954805e906c82b4bf3`. Demonstrated candidate policy blob: `1b8cc0a925321c66baf188e3ea0533652a2ee989`. The PR caption identifies the exact committed head; later caption/recording changes do not change this policy blob.
