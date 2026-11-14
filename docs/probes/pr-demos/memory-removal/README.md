# Memory command removal

The [recording](output.cast) captures actual CLI output from the installed
baseline (`99c777e5`, clean) and a candidate built from `7d3ba5d4` with this
removal. The candidate reports a dirty source build because qualification
preceded the commit. The final commit retains the qualified Rust behavior.

`memory --help` changes from accepted to an explicit unknown-command refusal.
Both calls request help only: no store, credentials or service is changed.
The recording contains one output frame; it does not measure command latency.
Replay with an asciinema v2 player, or read the JSON text as a fallback.

Repeat with absolute paths to the two executables:

```sh
python3 reproduce.py --before "$BASELINE_BINARY" --after "$CANDIDATE_BINARY"
```

Separately, `tests/ordinary_file_records.rs` verifies that existing Artifact
and RDF-value records reopen with exact source text intact and remain available
through ordinary query/CONSTRUCT. It also verifies refusal of the retired CLI
before store creation. Client memory policy and shapes belong to the client;
no production adoption, deletion or namespace migration is demonstrated here.
