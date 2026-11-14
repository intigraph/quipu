# One model command prepares existing facts

The recording compares the same isolated two-fact fixture on an installed CLI at
`f3982337b2211a28b8cf465faebad23ca51b28ee` and a default-feature candidate based
on `6c519be5c25dcb0d70fcbf2017a9c9482901bd8e` with this change. The candidate
reports a dirty build because the source was qualified before its commit. The
recorded behavior applies to the committed `cli_model.rs`, `model_backfill.rs`
and CLI test changes; generated help and this recording do not change it.

Both runs start with two ROOT labels and no model, configuration or runtime
override. They ingest cats and railway facts, confirm keyword search works, copy
verified pinned model assets into the isolated cache, then call
`quipu model install --db fresh.db` once. Watch readiness remain false with zero
vectors before, then become true with two vectors on the candidate. Its semantic
query ranks cats first. The output selects actual JSON fields and omits temporary
paths. Model acquisition and fresh Cargo installation are not demonstrated.

[Terminal recording](output.cast) uses the asciinema v2 JSON format. Output bytes
and frame timing are preserved from the original recording; CRLF is JSON-escaped
rather than stored as literal trailing whitespace. The playback viewport is
100 columns by 40 rows. Replay with the included [replay.py](replay.py):

```sh
python3 replay.py --speed 1 output.cast
```

Replay was checked against the original output bytes. Inline playback has not
been tested; download the recording and replay script. The JSON recording also
provides a text fallback.

Repeat each run using [probe.py](probe.py), an absolute executable path and the
verified model fixture directory:

```sh
python3 probe.py --binary "$QUIPU_BINARY" --model-dir "$QUIPU_TEST_MODEL_DIR"
```

The fixture requires `model.onnx` and `tokenizer.json` for MiniLM revision
`1110a243fdf4706b3f48f1d95db1a4f5529b4d41`; the probe verifies both SHA-256
checksums before writing its disposable HOME/database. Existing model assets are
copied only after ingest, so the explicit installer must prepare the facts.
The recording and caption are retained in this established probes directory.
