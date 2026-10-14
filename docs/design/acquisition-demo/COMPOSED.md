# Composed acquisition recording

This output-and-timing recording exercises a disposable private Router fixture
with the actual HTTP handlers, one MAIN project containing committed comments,
and one mounted immutable archive pack. It is candidate-source evidence; it does
not demonstrate a deployed service or configured SDK acceptance.

The shared request first succeeds through the opt-in `/acquire/composed` route
and is then refused by the retained `/acquire` v1 route. Watch the distinct
source receipt, provider exclusion, malformed pin, wrong generation, unknown
required ephemeral, and byte-cap responses. No refusal returns a receipt.

Recorded candidate base: `d8f755c3ac7442cdba53a18f2ed83e3594e6eda3`
(normal merge of main `ec3b707d` into the owned feature branch). The build
reports that base plus `producer_git_dirty=true`; the source content measured
in this recording is identified by the following SHA-256 values.

| Source | SHA-256 |
| --- | --- |
| `src/acquisition/composed.rs` | `e695eb7770bc433ae8c5f25ae1d44c560ad27ee66105937a0d08c1c462756898` |
| `src/acquisition/census.rs` | `26c4597187b6b8b6ce5946d8b14b9fd98a54a04b2430f28c5f62f1ed19fba62c` |
| `src/acquisition/main_graph.rs` | `2df5c188ce3b743769b8340d4a8853e7f6a7f7f2f288bac918e5b8b107c1dfb3` |
| `src/acquisition/provider_fence.rs` | `f4c4dd9dfa9d86be8f20e22ac32d7379ac722968668b65a69544a9a4fde1b7bb` |
| `src/store/freeze.rs` | `d7c48dec17cf16cd7ee6c819087bdb19ddf210ae2a7f741cef56ef84bebb3bc8` |
| `src/server/acquisition_tests.rs` | `edd001419c1b3ffa2ec47e4556ef788d40c7d72f494e2246578ae376c2369051` |

Download [output](composed-output.txt) and [timing](composed-timing.txt).
Replay with util-linux `scriptreplay`:

```sh
scriptreplay --log-out composed-output.txt --log-timing composed-timing.txt
```

This is a terminal recording, with a readable output transcript; no inline
playback is claimed. Asciinema was unavailable, so util-linux `script` recorded
actual filtered test output and its timing. Filtering hides compiler paths
and retains the actual requests, responses, test result, and HOME-guard result.
The output and timing files belong to this repository's existing small-demo
directory and are retained with the source.

Reproduce the fixture from the same source:

```sh
cargo test --features full --bin quipu-server composed_http_acquisition -- --nocapture
```

The historical and topology controls are separate library tests. Any explicit
MAIN `tx` on the new contract requires a previously observed generation token.
The new endpoint is an explicit opt-in; this demo does not claim that the old
endpoint now accepts attachments.
