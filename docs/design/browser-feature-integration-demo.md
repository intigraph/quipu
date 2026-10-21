# Browser feature integration demonstration

The [actual baseline and test transcript](assets/browser-features.txt) records
semantic search rejecting on the public page despite the full-feature WASM
bundle. The [candidate browser recording](assets/browser-features.webm) and
[full-page screenshot](assets/browser-features.png) show the workbench searching
an isolated two-document pack and ranking the feline above the automobile.
The recording is WebM; download it if the repository viewer does not play it.
These small artifacts remain in the repository with the change.

Product source: `8825b06e2c88a198bfbc1b8eda2f83d318451b82`, on base
`0f23ef3be3cc6ed32d62dacd4cf7c0986fc803bb`. Both baseline and candidate use
published Quipu 0.13.0 browser assets, worker version
`ec3b707dc2976e579e634f276e752c7715a9ac99`. No replacement WASM or model is
substituted. The candidate uses the real page, store worker and self-hosted
ONNX model; the graph is an explicitly synthetic local fixture.

Reproduce with the normal browser feature acceptance harness:

```bash
CHROMIUM_PATH=/path/to/chromium QUIPU_BIN=/path/to/quipu \
  node scripts/smoke-wasm-features.mjs /path/to/released-wasm \
  /path/to/released-wasm/runtime --headed
```

Install the repository-pinned browser test dependencies first. To record the
page, set `BROWSER_DEMO_DIR` to a private output directory; Playwright's video
encoder must also be installed in its test environment.

The assertions cover actual workbench UI, window API and relative HTTP search;
structured SHACL write refusals on both relays; a valid local write readback;
Datalog 0→1 and adopted OWL inverse reasoning; lazy model loading; federation
completeness, malformed members, label floors and timeouts. The fixture controls
have no remote graph or notification sink. Compiled flags alone are not counted
as functional acceptance. Existing WASM conformance evidence remains separate.

Changed page assets total 68,002→70,593 bytes, or 24,092→24,669 bytes when each
asset is gzipped deterministically. The unchanged released WASM is 4,420,860
bytes (1,717,264 gzipped), SHA-256
`3e5dc6814bf2d81d1e01ad84fa3b4d70eaf6909b0ea9b2544b4f21b882c93f8c`.
The heavy runtime and model remain lazy: the actual page makes zero requests
for them before the first search.

This proves candidate behavior with a controlled graph. Public Pages still
requires the normal reviewed merge and publication, followed by repeating the
served-page probe. It does not establish release, deployment or launch approval.
