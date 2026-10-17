# Endpoint pointer SERVICE acceptance

Before: Quipu 0.14.1 at `0cb06945` cannot use the endpoint declaration from
federation config; the CLI reports no configured remotes and sends no request.
After: the candidate verifies the local pendant and asks the real
Tambo/Caboodle endpoint decision CLI before every use. Exact consent returns one
actual SPARQL `SERVICE` row, with no credentials and no imported ROOT facts.
Foreign, pack-only and revoked consent cause no network request; URL neighbours,
redirects, forged declarations and malformed policy refuse. HTTP failures remain
errors. All requests and stores use temporary fixtures, with no live endpoints
or trust configuration changed. This is source acceptance, not installed rollout.

Build the candidate and fixture:

```bash
cargo build --features remote --bin quipu --example endpoint_pointer_fixture
python3 docs/design/endpoint-pointer-demo/demo.py /path/to/quipu /path/to/endpoint_pointer_fixture /path/to/caboodle --baseline /path/to/previous-quipu
```

The trust CLI must include Tambo PR168's `check-endpoint` command. The demo copies
executables into its temporary directory; it makes no symlinks to live tools.
It starts a loopback endpoint and closes it in `finally`.

`transcript.txt` is readable output; `output.gz` is the terminal
recording (command-header paths redacted; terminal payload preserved). Decompress it, then replay with
`scriptreplay --log-out output --log-timing timing.txt`.
