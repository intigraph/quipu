# Native operation attribution

This is a transcript of actual compiled parser tests, not a service capture or recording.
On base `e572cd0cc7b66260974513ee7668308f15b7d6a9`, the new regression sees
`uses-native-schema` become JSON null. The candidate retains all ten finite operation
names already emitted by the native reader. Unknown and repeated values remain null;
existing source, endpoint, status and client fields are preserved.

[Actual before/after transcript](output.log). The baseline is the base implementation
plus the same new tests. The candidate is the allowlist change and tests in this PR.

Reproduce the candidate with:

```sh
cargo test --bin quipu-server --features shacl,onnx,server request_command::tests -- --nocapture
```

The transcript is retained here as a small public review artifact. It does not prove
installed behavior, authenticate declared callers, change metric labels or establish
a request-rate reduction. Production log reads and service changes are outside this demo.
