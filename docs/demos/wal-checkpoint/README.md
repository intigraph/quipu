# WAL checkpoint writer isolation

Before this change, periodic WAL page copying and sync ran while the serving writer mutex was held. A large materialization qualification observed a concurrent write take 3.744 seconds while a checkpoint completed; the temporal overlap is a likely cause, not a separately timed checkpoint. The qualification failed its two-second maximum.

After this change, maintenance owns a separate checkpoint-only connection. The native concurrency control holds the serving writer mutex and requires an actual nonempty-WAL checkpoint to finish before that mutex is released. Refusal controls cover read-only handles, enrollment, sticky revocation after an external marker rollback, and identity changes. The checkpoint does not change the stored fixture value.

Recorded output: [native controls](output.log). This recording is from feature commit `6ed1dc4c`; the two feature blobs are unchanged by the subsequent base integration. Production-size qualification remains separate and is not claimed by this demonstration.

Reproduce from the repository root:

```sh
cargo test --locked --no-default-features --features shacl,onnx,owl,reactive-reasoner,server --lib store::wal::tests
cargo test --locked --no-default-features --features shacl,onnx,owl,reactive-reasoner,server --bin quipu-server owl_materialize
```

No production materialization switch or safety budget changes.
