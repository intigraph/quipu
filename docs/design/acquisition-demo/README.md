# Pin admission and acquisition demo

[Terminal recording](demo.cast) · [Readable transcript](transcript.txt)

The same `{"query":"ASK {}","tx":"bogus"}` input returns HTTP 200 and true
on the installed base, then is refused by the candidate's query library. The
unpinned `ASK {}` positive control remains true on both. The candidate then
acquires an assertion before and after its retraction/replacement, showing the
requested transaction, actual applied transaction, captured source head, exact
source build, graph bytes, and receipt digest. Altering the graph fails receipt
verification for every acquisition mode. The registered graph positive control
then passes and the same graph name used as a dataset alias is refused.

The [earlier alias probe](alias-baseline.txt) records the falsifier against clean
`8ca2e2c`: a project-only receipt contained the alias member’s comment data and
passed verification. The corrected candidate rejects that selection before
constructing a receipt; it does not broaden the requested graph scope.

Base: clean `222274080ee90e10723d4995a9f14172ced582dd`, observed through its HTTP
read interface. Candidate: clean source commit
`7b15592f4e5a0301827b830e761941f3b787f72a`, exercised through the actual compiled
library on a private in-memory fixture. The HTTP candidate's status and typed
receipt paths are separately covered by
`acquisition_http_contract_and_malformed_query_pin`. The recording compares the
shared query input across these two transports; its generated graph data is a
fixture, not a deployed source or configured consumer acceptance.

Reproduce the candidate controls with:

```sh
just test --lib acquisition::tests
just test --features server,shacl,onnx --bin quipu-server \
  acquisition_http_contract_and_malformed_query_pin
```

For playback, download `demo.cast` and run `asciinema play demo.cast`. The cast
preserves the actual terminal output and timing; it is not a video or an inline
embed. The original output/timing pair was replayed with `scriptreplay`, and
cast event output was verified byte-for-byte against that recording. Pause at
the end or use a slower replay speed to read the short acquisition sequence.

This small recording and transcript are retained in Git alongside the caption.
Source execution was guarded: 156 watched paths and five UID resources were
untouched, and the demonstration exited zero. Attached-store acquisition remains
refused; deployment and consumer activation require their separate acceptance.
