# Registry digest startup demonstration

Before: the added regression on base `360f8f49` observed one historical document
canonicalized during registry open; the test required zero and failed. The
retained native failure is in `before.txt`; it is historical output, not a live
execution of the old binary on replay.

After: replay runs the real native backfill controls, an actual private HTTP
listener callback check, invalid-stdio-credential refusal before readiness, and
a barrier test on the real server writer mutex.
The controls include restart, failed and oversized documents, same-version
replacement, rollback, current-version priority and exact pin identity.

Build development/full library and server test binaries with
`cargo test --features full --lib --bin quipu-server --no-run`. Set `TQA_LIB_TEST`
and `TQA_SERVER_TEST` to the emitted executable paths, then run
`python3 docs/probes/pr-demos/registry-digest-startup/run.py`.

`output.log` and `timing.log` record that replay. These are authoring test
artifacts, not clean release artifacts, a production-corpus benchmark, deployment
acceptance or a claimed cold-start speedup. Same-corpus/source-bound runtime
qualification remains a separate admitted check.
